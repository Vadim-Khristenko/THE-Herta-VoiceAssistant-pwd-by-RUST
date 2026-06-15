//! Единая иерархия ошибок. Никаких `panic!` в библиотечном коде — только `Result`.
//! Слои (LLM, инструменты, TUI, аудио) маппят свои ошибки в эти варианты.

use thiserror::Error;

/// Корневой тип ошибки ассистента.
#[derive(Debug, Error)]
pub enum HertaError {
    #[error("конфигурация: {0}")]
    Config(String),

    #[error("провайдер LLM `{provider}`: {message}")]
    Llm { provider: String, message: String },

    #[error("инструмент `{tool}`: {message}")]
    Tool { tool: String, message: String },

    #[error("саб-агент `{agent}`: {message}")]
    Agent { agent: String, message: String },

    #[error("аудио-подсистема: {0}")]
    Audio(String),

    #[error("рендеринг TUI: {0}")]
    Tui(String),

    #[error("память: {0}")]
    Memory(String),

    #[error("сеть: {0}")]
    Network(String),

    #[error("превышен лимит контекста: использовано {used} из {limit} токенов")]
    ContextOverflow { used: usize, limit: usize },

    #[error("ввод/вывод: {0}")]
    Io(#[from] std::io::Error),

    #[error("сериализация JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

impl HertaError {
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    pub fn llm(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Llm { provider: provider.into(), message: message.into() }
    }

    pub fn tool(tool: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Tool { tool: tool.into(), message: message.into() }
    }

    pub fn agent(agent: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Agent { agent: agent.into(), message: message.into() }
    }
}

/// Сокращение для результатов библиотеки.
pub type Result<T> = std::result::Result<T, HertaError>;
