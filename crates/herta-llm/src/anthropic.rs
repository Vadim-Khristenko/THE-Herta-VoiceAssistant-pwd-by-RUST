//! Клиент Anthropic Messages API (Claude). Официального Rust SDK нет, поэтому
//! используем сырой HTTP через reqwest — это явно разрешённый путь для языков
//! без SDK.
//!
//! Особенности протокола относительно OpenAI:
//! - системный промпт идёт отдельным полем `system`, а не ролью в `messages`;
//! - роли только `user`/`assistant`; результат инструмента — это `user`-сообщение
//!   с блоком `tool_result`, а вызов — блок `tool_use` в ответе ассистента;
//! - для моделей 4.x запрещены `temperature`/`top_p` (иначе HTTP 400), поэтому
//!   стиль задаётся исключительно промптом-персоной.

use crate::retry::{is_retryable_status, with_backoff};
use crate::{sanitize_reply, ChatClient, ChatResponse};
use async_trait::async_trait;
use herta_core::config::AnthropicConfig;
use herta_core::{HertaError, Message, Result, Role, ToolCall, ToolSpec};
use serde_json::{json, Value};
use std::time::Duration;

pub struct AnthropicClient {
    config: AnthropicConfig,
    http: reqwest::Client,
}

impl std::fmt::Debug for AnthropicClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicClient")
            .field("model", &self.config.model)
            .finish()
    }
}

impl AnthropicClient {
    pub fn new(config: AnthropicConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs_f64(config.timeout_seconds))
            .build()
            .map_err(|e| HertaError::llm("anthropic", e.to_string()))?;
        Ok(Self { config, http })
    }

    fn endpoint(&self) -> String {
        format!("{}/messages", self.config.base_url.trim_end_matches('/'))
    }

    /// Делит историю на системную инструкцию и массив сообщений `user`/`assistant`.
    fn split_payload(messages: &[Message]) -> (Option<String>, Vec<Value>) {
        let mut system_parts: Vec<String> = Vec::new();
        let mut out: Vec<Value> = Vec::new();
        for m in messages {
            match m.role {
                Role::System => system_parts.push(m.content.clone()),
                Role::Assistant => out.push(json!({ "role": "assistant", "content": m.content })),
                // Tool-результаты в провайдер-агностичном цикле приходят как обычный
                // текст пользователя — это валидно для Anthropic.
                Role::User | Role::Tool => {
                    out.push(json!({ "role": "user", "content": m.content }))
                }
            }
        }
        let system = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        };
        (system, out)
    }

    fn tool_schemas(tools: &[ToolSpec]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                let mut properties = serde_json::Map::new();
                let mut required = Vec::new();
                for p in &t.parameters {
                    properties.insert(
                        p.name.clone(),
                        json!({ "type": p.param_type.json_type(), "description": p.description }),
                    );
                    if p.required {
                        required.push(Value::String(p.name.clone()));
                    }
                }
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": {
                        "type": "object",
                        "properties": Value::Object(properties),
                        "required": required,
                    }
                })
            })
            .collect()
    }

    async fn post(&self, body: Value) -> Result<Value> {
        let key = self
            .config
            .api_key
            .clone()
            .ok_or_else(|| HertaError::llm("anthropic", "не задан ANTHROPIC_API_KEY"))?;

        with_backoff(self.config.retry_attempts + 1, || {
            let http = &self.http;
            let endpoint = self.endpoint();
            let key = key.clone();
            let version = self.config.api_version.clone();
            let body = body.clone();
            async move {
                let resp = http
                    .post(&endpoint)
                    .header("x-api-key", key)
                    .header("anthropic-version", version)
                    .header("content-type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| (true, HertaError::Network(e.to_string())))?;
                let status = resp.status().as_u16();
                if status == 200 {
                    let value: Value = resp
                        .json()
                        .await
                        .map_err(|e| (false, HertaError::llm("anthropic", e.to_string())))?;
                    return Ok(value);
                }
                let retryable = is_retryable_status(status);
                let text = resp.text().await.unwrap_or_default();
                Err((
                    retryable,
                    HertaError::llm("anthropic", format!("HTTP {status}: {text}")),
                ))
            }
        })
        .await
    }

    /// Собирает текст из блоков `content` типа `text` и чистит `<think>`.
    fn extract_text(value: &Value) -> String {
        let Some(blocks) = value.get("content").and_then(Value::as_array) else {
            return String::new();
        };
        let joined: String = blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("");
        sanitize_reply(&joined)
    }

    fn extract_tool_calls(value: &Value) -> Vec<ToolCall> {
        let Some(blocks) = value.get("content").and_then(Value::as_array) else {
            return Vec::new();
        };
        blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
            .filter_map(|b| {
                let id = b
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                let name = b.get("name").and_then(Value::as_str)?.to_string();
                let arguments = b.get("input").cloned().unwrap_or_else(|| json!({}));
                Some(ToolCall {
                    id,
                    name,
                    arguments,
                })
            })
            .collect()
    }

    fn base_body(&self, messages: &[Message]) -> Value {
        let (system, msgs) = Self::split_payload(messages);
        let mut body = json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens,
            "messages": msgs,
        });
        if let Some(sys) = system {
            body["system"] = json!(sys);
        }
        body
    }
}

#[async_trait]
impl ChatClient for AnthropicClient {
    fn provider_name(&self) -> &'static str {
        "anthropic"
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    async fn warm_up(&self) -> Result<()> {
        if self.config.api_key.is_none() {
            return Err(HertaError::llm("anthropic", "не задан ANTHROPIC_API_KEY"));
        }
        Ok(())
    }

    async fn chat(&self, messages: &[Message]) -> Result<String> {
        let value = self.post(self.base_body(messages)).await?;
        Ok(Self::extract_text(&value))
    }

    async fn chat_with_tools(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
    ) -> Result<ChatResponse> {
        if tools.is_empty() {
            return Ok(ChatResponse::text(self.chat(messages).await?));
        }
        let mut body = self.base_body(messages);
        body["tools"] = json!(Self::tool_schemas(tools));
        let value = self.post(body).await?;
        Ok(ChatResponse {
            text: Self::extract_text(&value),
            tool_calls: Self::extract_tool_calls(&value),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_extracts_system() {
        let msgs = vec![
            Message::system("персона"),
            Message::user("привет"),
            Message::assistant("Уже лучше."),
            Message::tool("call_1", "результат"),
        ];
        let (system, out) = AnthropicClient::split_payload(&msgs);
        assert_eq!(system.as_deref(), Some("персона"));
        assert_eq!(out.len(), 3);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[1]["role"], "assistant");
        assert_eq!(out[2]["role"], "user"); // tool → user
    }

    #[test]
    fn parses_tool_use_blocks() {
        let value = json!({
            "content": [
                {"type": "text", "text": "Считаю."},
                {"type": "tool_use", "id": "toolu_1", "name": "calc", "input": {"x": 2}}
            ]
        });
        assert_eq!(AnthropicClient::extract_text(&value), "Считаю.");
        let calls = AnthropicClient::extract_tool_calls(&value);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "calc");
        assert_eq!(calls[0].arguments["x"], 2);
    }
}
