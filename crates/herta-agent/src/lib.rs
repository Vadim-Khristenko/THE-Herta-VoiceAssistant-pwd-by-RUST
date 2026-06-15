//! `herta-agent` — оркестрация саб-агентов.
//!
//! «Марионетки» Герты: каждый саб-агент решает узкую задачу в отдельной
//! `tokio`-таске и стримит события в общий канал. Главный поток (TUI) только
//! читает канал, поэтому массовые параллельные обновления не блокируют рендер.
//!
//! Параллелизм ограничен семафором (`max_concurrent`), каждый агент — таймаутом.

#![forbid(unsafe_code)]

use herta_core::{Message, ToolResult};
use herta_core::config::AgentConfig;
use herta_llm::ChatClient;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Semaphore};

/// Задача для саб-агента.
#[derive(Debug, Clone)]
pub struct AgentTask {
    pub id: String,
    pub title: String,
    pub prompt: String,
}

impl AgentTask {
    pub fn new(title: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self { id: uuid::Uuid::new_v4().to_string(), title: title.into(), prompt: prompt.into() }
    }
}

/// Событие жизненного цикла саб-агента, отправляемое в канал.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Started { id: String, title: String },
    /// Инкрементальный фрагмент вывода (для живого отображения).
    Chunk { id: String, text: String },
    Completed { id: String, output: String },
    Failed { id: String, error: String },
}

impl AgentEvent {
    pub fn id(&self) -> &str {
        match self {
            AgentEvent::Started { id, .. }
            | AgentEvent::Chunk { id, .. }
            | AgentEvent::Completed { id, .. }
            | AgentEvent::Failed { id, .. } => id,
        }
    }
}

/// Снимок состояния одного саб-агента для отображения в TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Pending,
    Running,
    Done,
    Error,
}

/// Супервизор саб-агентов. Клонируемый: внутри `Arc`.
#[derive(Clone)]
pub struct Supervisor {
    client: Arc<dyn ChatClient>,
    system_prompt: Arc<String>,
    semaphore: Arc<Semaphore>,
    timeout: Duration,
}

impl std::fmt::Debug for Supervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Supervisor")
            .field("provider", &self.client.provider_name())
            .field("available_permits", &self.semaphore.available_permits())
            .finish()
    }
}

impl Supervisor {
    pub fn new(client: Arc<dyn ChatClient>, config: &AgentConfig, system_prompt: impl Into<String>) -> Self {
        Self {
            client,
            system_prompt: Arc::new(system_prompt.into()),
            semaphore: Arc::new(Semaphore::new(config.max_concurrent.max(1))),
            timeout: Duration::from_secs(config.timeout_seconds.max(1)),
        }
    }

    /// Запустить одну задачу. События уходят в `tx`. Возвращает `JoinHandle`.
    pub fn spawn(&self, task: AgentTask, tx: mpsc::UnboundedSender<AgentEvent>) -> tokio::task::JoinHandle<()> {
        let client = Arc::clone(&self.client);
        let system_prompt = Arc::clone(&self.system_prompt);
        let semaphore = Arc::clone(&self.semaphore);
        let timeout = self.timeout;

        tokio::spawn(async move {
            // Ограничение параллелизма: ждём свободный слот.
            let _permit = match semaphore.acquire().await {
                Ok(p) => p,
                Err(_) => {
                    let _ = tx.send(AgentEvent::Failed { id: task.id.clone(), error: "семафор закрыт".into() });
                    return;
                }
            };

            let _ = tx.send(AgentEvent::Started { id: task.id.clone(), title: task.title.clone() });

            let messages = vec![Message::system(system_prompt.as_str()), Message::user(task.prompt.clone())];
            let work = client.chat(&messages);

            match tokio::time::timeout(timeout, work).await {
                Ok(Ok(output)) => {
                    let _ = tx.send(AgentEvent::Completed { id: task.id, output });
                }
                Ok(Err(e)) => {
                    let _ = tx.send(AgentEvent::Failed { id: task.id, error: e.to_string() });
                }
                Err(_) => {
                    let _ = tx.send(AgentEvent::Failed { id: task.id, error: "таймаут саб-агента".into() });
                }
            }
        })
    }

    /// Запустить пачку задач параллельно. Возвращает приёмник событий.
    pub fn spawn_batch(&self, tasks: Vec<AgentTask>) -> mpsc::UnboundedReceiver<AgentEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        for task in tasks {
            self.spawn(task, tx.clone());
        }
        rx
    }

    /// Собрать результаты пачки в порядке завершения. Удобно вне TUI.
    pub async fn run_to_completion(&self, tasks: Vec<AgentTask>) -> Vec<ToolResult> {
        let expected = tasks.len();
        let mut rx = self.spawn_batch(tasks);
        let mut results = Vec::with_capacity(expected);
        let mut finished = 0;
        while finished < expected {
            match rx.recv().await {
                Some(AgentEvent::Completed { id, output }) => {
                    results.push(ToolResult::ok(format!("agent:{id}"), output));
                    finished += 1;
                }
                Some(AgentEvent::Failed { id, error }) => {
                    results.push(ToolResult::rejected(format!("agent:{id}"), error));
                    finished += 1;
                }
                Some(_) => {}
                None => break,
            }
        }
        results
    }
}
