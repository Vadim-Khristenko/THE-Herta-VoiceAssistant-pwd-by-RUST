//! Изменчивое состояние интерфейса. Отделено от структурной конфигурации
//! (темы) и от бэкенд-клиентов, чтобы рендер не конфликтовал с `&mut Frame`.

use herta_agent::AgentStatus;

/// Кто автор строки в ленте диалога.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    User,
    Herta,
    Notice,
    ErrorNote,
}

/// Одна реплика в ленте.
#[derive(Debug, Clone)]
pub struct ChatLine {
    pub kind: LineKind,
    pub text: String,
}

impl ChatLine {
    pub fn user(text: impl Into<String>) -> Self {
        Self { kind: LineKind::User, text: text.into() }
    }
    pub fn herta(text: impl Into<String>) -> Self {
        Self { kind: LineKind::Herta, text: text.into() }
    }
    pub fn notice(text: impl Into<String>) -> Self {
        Self { kind: LineKind::Notice, text: text.into() }
    }
    pub fn error(text: impl Into<String>) -> Self {
        Self { kind: LineKind::ErrorNote, text: text.into() }
    }
}

/// Отображаемое состояние одного саб-агента.
#[derive(Debug, Clone)]
pub struct AgentView {
    pub id: String,
    pub title: String,
    pub status: AgentStatus,
    pub preview: String,
}

/// Какая панель в фокусе ввода.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Input,
    Transcript,
}

/// Корневое изменчивое состояние приложения.
#[derive(Debug)]
pub struct AppState {
    pub lines: Vec<ChatLine>,
    pub input: String,
    pub agents: Vec<AgentView>,
    pub status: String,
    pub focus: Focus,
    /// Прокрутка ленты: число строк, скрытых снизу (0 = прижато к низу).
    pub scroll_back: u16,
    /// Идёт ли сейчас запрос к модели.
    pub busy: bool,
    pub should_quit: bool,
    pub provider_label: String,
    pub model_label: String,
    /// Оценка занятости контекстного окна, токены.
    pub context_used: usize,
    pub context_limit: usize,
}

impl AppState {
    pub fn new(provider_label: impl Into<String>, model_label: impl Into<String>, context_limit: usize) -> Self {
        Self {
            lines: vec![ChatLine::notice(
                "Великая Герта на связи. Печатайте запрос и жмите Enter. F1 — справка.",
            )],
            input: String::new(),
            agents: Vec::new(),
            status: "готова".into(),
            focus: Focus::Input,
            scroll_back: 0,
            busy: false,
            should_quit: false,
            provider_label: provider_label.into(),
            model_label: model_label.into(),
            context_used: 0,
            context_limit: context_limit.max(1),
        }
    }

    pub fn push_line(&mut self, line: ChatLine) {
        self.lines.push(line);
        // Новое сообщение — возвращаемся к низу ленты.
        self.scroll_back = 0;
    }

    /// Обновить или вставить представление агента по событию.
    pub fn upsert_agent(&mut self, id: &str, title: Option<String>, status: AgentStatus, preview: Option<String>) {
        if let Some(agent) = self.agents.iter_mut().find(|a| a.id == id) {
            agent.status = status;
            if let Some(t) = title {
                agent.title = t;
            }
            if let Some(p) = preview {
                agent.preview = p;
            }
        } else {
            self.agents.push(AgentView {
                id: id.to_string(),
                title: title.unwrap_or_else(|| "агент".into()),
                status,
                preview: preview.unwrap_or_default(),
            });
        }
    }

    pub fn context_ratio(&self) -> f32 {
        (self.context_used as f32 / self.context_limit as f32).clamp(0.0, 1.0)
    }
}
