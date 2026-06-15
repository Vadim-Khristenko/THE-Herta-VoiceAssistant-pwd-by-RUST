//! Управляющий цикл TUI. Единый `tokio::select!`-селектор сводит три источника:
//! события терминала (crossterm `EventStream`), ответы модели и поток событий
//! саб-агентов. Главный поток только читает каналы и рендерит — тяжёлая работа
//! (запросы к модели, нативный tool-loop, саб-агенты) уходит в отдельные таски,
//! поэтому интерфейс не блокируется.

use crate::state::{AppState, ChatLine, Focus};
use crate::theme::Theme;
use crate::ui;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use herta_agent::{run_tool_loop, AgentEvent, AgentStatus, AgentTask, Supervisor};
use herta_core::persona;
use herta_core::{
    estimate_total_tokens, CompactionDecision, CompactionPlan, ContextManager, HertaError, Message,
    Result,
};
use herta_llm::ChatClient;
use herta_tools::ToolRegistry;
use herta_voice::Voice;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Внутренние события от фоновых задач к управляющему циклу.
#[derive(Debug)]
enum Backend {
    Reply(String),
    ReplyError(String),
    Summary { plan: CompactionPlan, text: String },
    SummaryError(String),
}

/// Структурные зависимости приложения (не изменяются по кадрам).
pub struct App {
    state: AppState,
    theme: Theme,
    client: Arc<dyn ChatClient>,
    registry: Arc<ToolRegistry>,
    supervisor: Supervisor,
    ctx_manager: ContextManager,
    voice: Voice,
    conversation: Vec<Message>,
    /// Текущая цель пользователя (команда /goal), инъектируется в каждый запрос.
    goal: Option<String>,
    /// Предел итераций нативного tool-loop.
    tool_iterations: usize,
    show_help: bool,
    backend_tx: mpsc::UnboundedSender<Backend>,
    backend_rx: mpsc::UnboundedReceiver<Backend>,
    agent_tx: mpsc::UnboundedSender<AgentEvent>,
    agent_rx: mpsc::UnboundedReceiver<AgentEvent>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: Arc<dyn ChatClient>,
        registry: Arc<ToolRegistry>,
        supervisor: Supervisor,
        ctx_manager: ContextManager,
        voice: Voice,
        context_limit: usize,
        tool_iterations: usize,
        long_memory_block: Option<String>,
    ) -> Self {
        let provider = client.provider_name().to_string();
        let model = client.model_name().to_string();
        let conversation =
            persona::build_bootstrap_messages(Some(&model), long_memory_block.as_deref());

        let (backend_tx, backend_rx) = mpsc::unbounded_channel();
        let (agent_tx, agent_rx) = mpsc::unbounded_channel();

        let mut state = AppState::new(provider, model, context_limit);
        state.context_used = estimate_total_tokens(&conversation);
        let tool_count = registry.len();
        if tool_count > 0 {
            state.push_line(ChatLine::notice(format!(
                "Доступно инструментов: {tool_count}. Команды: /goal /ask /tools /compact /model /help."
            )));
        }

        Self {
            state,
            theme: Theme::default(),
            client,
            registry,
            supervisor,
            ctx_manager,
            voice,
            conversation,
            goal: None,
            tool_iterations: tool_iterations.max(1),
            show_help: false,
            backend_tx,
            backend_rx,
            agent_tx,
            agent_rx,
        }
    }

    /// Запустить интерфейс. Восстанавливает терминал при любом исходе.
    pub async fn run(mut self) -> Result<()> {
        let mut terminal = setup_terminal()?;
        let result = self.event_loop(&mut terminal).await;
        restore_terminal(&mut terminal)?;
        result
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> Result<()> {
        let mut events = EventStream::new();
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(120));

        self.draw(terminal)?;

        while !self.state.should_quit {
            tokio::select! {
                maybe_event = events.next() => {
                    match maybe_event {
                        Some(Ok(event)) => self.handle_terminal_event(event),
                        Some(Err(e)) => self.state.status = format!("ошибка ввода: {e}"),
                        None => self.state.should_quit = true,
                    }
                }
                Some(msg) = self.backend_rx.recv() => self.handle_backend(msg),
                Some(ev) = self.agent_rx.recv() => self.handle_agent_event(ev),
                _ = ticker.tick() => {}
            }
            self.draw(terminal)?;
        }
        Ok(())
    }

    fn draw(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        terminal
            .draw(|frame| {
                ui::render(frame, &self.state, &self.theme);
                if self.show_help {
                    ui::render_help(frame, &self.theme);
                }
            })
            .map_err(|e| HertaError::Tui(e.to_string()))?;
        Ok(())
    }

    fn handle_terminal_event(&mut self, event: Event) {
        let Event::Key(key) = event else { return };
        if key.kind != KeyEventKind::Press {
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            self.state.should_quit = true;
            return;
        }
        if self.show_help {
            if matches!(key.code, KeyCode::Esc | KeyCode::F(1)) {
                self.show_help = false;
            }
            return;
        }
        self.handle_key(key);
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::F(1) => self.show_help = true,
            KeyCode::Tab => {
                self.state.focus = match self.state.focus {
                    Focus::Input => Focus::Transcript,
                    Focus::Transcript => Focus::Input,
                };
            }
            KeyCode::PageUp => self.state.scroll_back = self.state.scroll_back.saturating_add(3),
            KeyCode::PageDown => self.state.scroll_back = self.state.scroll_back.saturating_sub(3),
            KeyCode::Enter => self.submit(),
            KeyCode::Backspace => {
                self.state.input.pop();
            }
            // Слияние ввода и условия в один match-arm — clippy::collapsible_match.
            KeyCode::Char(c) if self.state.focus == Focus::Input => self.state.input.push(c),
            _ => {}
        }
    }

    fn submit(&mut self) {
        if self.state.busy {
            return;
        }
        let text = self.state.input.trim().to_string();
        self.state.input.clear();
        if text.is_empty() {
            return;
        }
        if let Some(rest) = text.strip_prefix('/') {
            self.handle_command(rest.trim());
            return;
        }

        // Быстрый ответ об идентичности без обращения к модели.
        if persona::is_identity_query(&text) {
            let reply = persona::build_identity_reply(&text);
            self.state.push_line(ChatLine::user(text.clone()));
            self.state.push_line(ChatLine::herta(reply.clone()));
            self.conversation.push(Message::user(text));
            self.conversation.push(Message::assistant(reply));
            self.recompute_context();
            return;
        }

        self.state.push_line(ChatLine::user(text.clone()));
        self.conversation.push(Message::user(text));
        self.recompute_context();
        self.dispatch_turn();
    }

    /// Отправить текущий контекст в модель через нативный tool-loop.
    fn dispatch_turn(&mut self) {
        let mut request = self.conversation.clone();
        let last_user = request
            .iter()
            .rev()
            .find(|m| matches!(m.role, herta_core::Role::User))
            .map(|m| m.content.clone())
            .unwrap_or_default();
        if let Some(hint) = persona::build_conversational_hint(&last_user) {
            request.push(Message::system(hint));
        }
        if let Some(goal) = &self.goal {
            request.push(Message::system(format!(
                "Текущая цель пользователя: {goal}. Держи её в фокусе."
            )));
        }

        self.state.busy = true;
        self.state.status = "запрос к модели…".into();

        let client = Arc::clone(&self.client);
        let registry = Arc::clone(&self.registry);
        let tx = self.backend_tx.clone();
        let iters = self.tool_iterations;
        tokio::spawn(async move {
            let msg = match run_tool_loop(client.as_ref(), registry.as_ref(), &request, iters).await
            {
                Ok(outcome) => Backend::Reply(outcome.text),
                Err(e) => Backend::ReplyError(e.to_string()),
            };
            let _ = tx.send(msg);
        });
    }

    fn handle_command(&mut self, command: &str) {
        let (head, tail) = command
            .split_once(char::is_whitespace)
            .unwrap_or((command, ""));
        let tail = tail.trim();
        match head {
            "quit" | "q" | "exit" => self.state.should_quit = true,
            "help" | "h" => self.show_help = true,
            "clear" => {
                self.state.lines.clear();
                self.state.push_line(ChatLine::notice("Лента очищена."));
            }
            "model" => {
                self.state.push_line(ChatLine::notice(format!(
                    "Провайдер: {} · модель: {} · контекст: {}/{} токенов",
                    self.state.provider_label,
                    self.state.model_label,
                    self.state.context_used,
                    self.state.context_limit
                )));
            }
            "tools" => {
                let mut specs = self.registry.specs();
                specs.sort_by(|a, b| a.name.cmp(&b.name));
                if specs.is_empty() {
                    self.state
                        .push_line(ChatLine::notice("Инструменты не подключены."));
                } else {
                    let listing = specs
                        .iter()
                        .map(|s| {
                            format!(
                                "• {} — {}",
                                s.name,
                                s.description.lines().next().unwrap_or("")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.state.push_line(ChatLine::notice(format!(
                        "Инструменты ({}):\n{listing}",
                        specs.len()
                    )));
                }
            }
            "goal" => {
                if tail.is_empty() {
                    match &self.goal {
                        Some(g) => self
                            .state
                            .push_line(ChatLine::notice(format!("Текущая цель: {g}"))),
                        None => self
                            .state
                            .push_line(ChatLine::notice("Использование: /goal <описание цели>")),
                    }
                } else {
                    self.goal = Some(tail.to_string());
                    self.state
                        .push_line(ChatLine::notice(format!("Цель установлена: {tail}")));
                    // Сразу просим Герту составить план через навык goal-planning.
                    let planning = format!(
                        "Моя цель: {tail}. Используй навык goal-planning (list_skills/use_skill), составь план и начни."
                    );
                    self.state.push_line(ChatLine::user(planning.clone()));
                    self.conversation.push(Message::user(planning));
                    self.recompute_context();
                    self.dispatch_turn();
                }
            }
            "ask" => {
                if tail.is_empty() {
                    self.state
                        .push_line(ChatLine::notice("Использование: /ask <вопрос саб-агенту>"));
                } else {
                    self.spawn_agent("вопрос", tail);
                }
            }
            "agent" => {
                if tail.is_empty() {
                    self.state
                        .push_line(ChatLine::notice("Использование: /agent <описание задачи>"));
                } else {
                    self.spawn_agent("задача", tail);
                }
            }
            "compact" => self.force_compact(),
            "say" => {
                if !self.voice.is_available() {
                    self.state.push_line(ChatLine::notice(
                        "TTS недоступен (нет say/espeak/powershell).",
                    ));
                } else if tail.is_empty() {
                    self.state
                        .push_line(ChatLine::notice("Использование: /say <текст для озвучки>"));
                } else {
                    self.voice.speak(tail);
                    self.state.push_line(ChatLine::notice("Озвучиваю."));
                }
            }
            other => self
                .state
                .push_line(ChatLine::notice(format!("Неизвестная команда: /{other}"))),
        }
    }

    fn spawn_agent(&mut self, kind: &str, task_text: &str) {
        let title: String = task_text.chars().take(28).collect();
        let task = AgentTask::new(title.clone(), task_text.to_string());
        self.state
            .upsert_agent(&task.id, Some(title), AgentStatus::Pending, None);
        self.state.push_line(ChatLine::notice(format!(
            "Марионетка ({kind}): {task_text}"
        )));
        self.supervisor.spawn(task, self.agent_tx.clone());
    }

    fn handle_backend(&mut self, msg: Backend) {
        match msg {
            Backend::Reply(reply) => {
                let reply = if reply.trim().is_empty() {
                    "Пустой ответ модели. Уточните запрос.".to_string()
                } else {
                    reply
                };
                if persona::needs_persona_repair(&reply) {
                    self.state.status = "персона under repair".into();
                }
                if self.voice.is_enabled() {
                    self.voice.speak(&reply);
                }
                self.state.push_line(ChatLine::herta(reply.clone()));
                self.conversation.push(Message::assistant(reply));
                self.state.busy = false;
                self.state.status = "готова".into();
                self.recompute_context();
                self.maybe_compact();
            }
            Backend::ReplyError(err) => {
                self.state
                    .push_line(ChatLine::error(format!("Сбой запроса: {err}")));
                self.state.busy = false;
                self.state.status = "ошибка".into();
            }
            Backend::Summary { plan, text } => {
                self.conversation = ContextManager::apply(&self.conversation, &plan, &text);
                self.state.busy = false;
                self.state.status = "контекст сжат".into();
                self.state
                    .push_line(ChatLine::notice("Контекст сжат для экономии окна."));
                self.recompute_context();
            }
            Backend::SummaryError(err) => {
                self.state.busy = false;
                self.state.status = format!("сжатие не удалось: {err}");
            }
        }
    }

    fn handle_agent_event(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::Started { id, title } => {
                self.state.upsert_agent(
                    &id,
                    Some(title),
                    AgentStatus::Running,
                    Some("работает…".into()),
                );
            }
            AgentEvent::Chunk { id, text } => {
                self.state
                    .upsert_agent(&id, None, AgentStatus::Running, Some(preview(&text)));
            }
            AgentEvent::Completed { id, output } => {
                self.state
                    .upsert_agent(&id, None, AgentStatus::Done, Some(preview(&output)));
                self.state
                    .push_line(ChatLine::herta(format!("[марионетка] {output}")));
            }
            AgentEvent::Failed { id, error } => {
                self.state
                    .upsert_agent(&id, None, AgentStatus::Error, Some(preview(&error)));
                self.state
                    .push_line(ChatLine::error(format!("[марионетка] сбой: {error}")));
            }
        }
    }

    fn recompute_context(&mut self) {
        self.state.context_used = estimate_total_tokens(&self.conversation);
    }

    /// Автосжатие при достижении порога.
    fn maybe_compact(&mut self) {
        if self.state.busy {
            return;
        }
        if let CompactionDecision::Compact(plan) = self.ctx_manager.decide(&self.conversation) {
            self.run_compaction(plan);
        }
    }

    /// Принудительное сжатие по команде /compact.
    fn force_compact(&mut self) {
        if self.state.busy {
            self.state
                .push_line(ChatLine::notice("Занята; сжатие после ответа."));
            return;
        }
        match self.ctx_manager.force_plan(&self.conversation) {
            Some(plan) => self.run_compaction(plan),
            None => self
                .state
                .push_line(ChatLine::notice("Недостаточно истории для сжатия.")),
        }
    }

    fn run_compaction(&mut self, plan: CompactionPlan) {
        let request = ContextManager::build_summarization_request(&self.conversation, &plan);
        self.state.busy = true;
        self.state.status = "сжимаю контекст…".into();
        let client = Arc::clone(&self.client);
        let tx = self.backend_tx.clone();
        tokio::spawn(async move {
            let msg = match client.chat(&request).await {
                Ok(text) => Backend::Summary { plan, text },
                Err(e) => Backend::SummaryError(e.to_string()),
            };
            let _ = tx.send(msg);
        });
    }
}

/// Усечь текст для превью в панели агентов без выхода за границы символов.
fn preview(text: &str) -> String {
    let one_line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    one_line.chars().take(60).collect()
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().map_err(|e| HertaError::Tui(e.to_string()))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| HertaError::Tui(e.to_string()))?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).map_err(|e| HertaError::Tui(e.to_string()))
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode().map_err(|e| HertaError::Tui(e.to_string()))?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .map_err(|e| HertaError::Tui(e.to_string()))?;
    terminal
        .show_cursor()
        .map_err(|e| HertaError::Tui(e.to_string()))?;
    Ok(())
}
