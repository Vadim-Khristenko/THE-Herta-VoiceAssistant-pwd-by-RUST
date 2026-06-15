//! Базовые типы диалога: роли, сообщения, токен-оценка.
//! Совместимы с OpenAI-подобным форматом и нативными протоколами провайдеров.

use serde::{Deserialize, Serialize};

/// Роль автора сообщения в диалоге.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    /// Результат выполнения инструмента, возвращаемый модели.
    Tool,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// Одно сообщение диалога. `tool_call_id` заполняется только для `Role::Tool`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: Role::System, content: content.into(), name: None, tool_call_id: None }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into(), name: None, tool_call_id: None }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: content.into(), name: None, tool_call_id: None }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            name: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    /// Грубая оценка числа токенов. Эвристика ~4 символа/токен для смешанного
    /// кириллица/латиница текста. Для точных бюджетов провайдер уточняет сам.
    pub fn estimate_tokens(&self) -> usize {
        estimate_tokens(&self.content) + 4 // накладные расходы на роль/разметку
    }
}

/// Эвристическая оценка токенов в произвольной строке.
/// Кириллица расходует больше байт, поэтому делим символы, а не байты.
pub fn estimate_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    chars.div_ceil(4).max(1)
}

/// Оценка суммарного бюджета токенов для среза сообщений.
pub fn estimate_total_tokens(messages: &[Message]) -> usize {
    messages.iter().map(Message::estimate_tokens).sum()
}
