//! Примитивы вызова инструментов, общие для слоёв LLM и инструментов.
//! Живут в `core`, чтобы и провайдеры, и реестр инструментов зависели от одних
//! типов без циклов в графе зависимостей.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Тип параметра инструмента в схеме function-calling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    String,
    Integer,
    Number,
    Boolean,
}

impl ParamType {
    pub fn json_type(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
        }
    }
}

/// Один параметр инструмента.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameter {
    pub name: String,
    pub param_type: ParamType,
    pub description: String,
    pub required: bool,
}

impl ToolParameter {
    pub fn new(name: impl Into<String>, param_type: ParamType, description: impl Into<String>, required: bool) -> Self {
        Self { name: name.into(), param_type, description: description.into(), required }
    }
}

/// Описание инструмента (схема) для модели.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ToolParameter>,
    /// Деструктивные инструменты по умолчанию блокируются реестром.
    pub destructive: bool,
}

impl ToolSpec {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Vec<ToolParameter>) -> Self {
        Self { name: name.into(), description: description.into(), parameters, destructive: false }
    }

    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// JSON-схема параметров в стиле OpenAI function-calling.
    pub fn to_json_schema(&self) -> Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();
        for p in &self.parameters {
            properties.insert(
                p.name.clone(),
                serde_json::json!({ "type": p.param_type.json_type(), "description": p.description }),
            );
            if p.required {
                required.push(Value::String(p.name.clone()));
            }
        }
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": {
                    "type": "object",
                    "properties": Value::Object(properties),
                    "required": required,
                }
            }
        })
    }
}

/// Запрос модели на вызов инструмента.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Идентификатор вызова (для сопоставления с ответом-`tool`).
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

impl ToolCall {
    pub fn arg_str(&self, key: &str) -> Option<String> {
        self.arguments.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
    }

    pub fn arg_bool(&self, key: &str) -> Option<bool> {
        self.arguments.get(key).and_then(Value::as_bool)
    }
}

/// Результат выполнения инструмента.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub message: String,
    pub executed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ToolResult {
    pub fn ok(tool_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self { tool_name: tool_name.into(), message: message.into(), executed: true, data: None }
    }

    pub fn rejected(tool_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self { tool_name: tool_name.into(), message: message.into(), executed: false, data: None }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}
