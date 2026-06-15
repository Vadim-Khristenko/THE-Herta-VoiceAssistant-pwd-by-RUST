//! Локальный/удалённый клиент Ollama через HTTP `/api/chat`.

use crate::retry::{is_retryable_status, with_backoff};
use crate::{sanitize_reply, ChatClient, ChatResponse};
use async_trait::async_trait;
use herta_core::config::OllamaConfig;
use herta_core::{HertaError, Message, Result, ToolCall, ToolSpec};
use serde_json::{json, Value};
use std::time::Duration;

pub struct OllamaClient {
    config: OllamaConfig,
    http: reqwest::Client,
}

impl std::fmt::Debug for OllamaClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OllamaClient").field("host", &self.config.host).field("model", &self.config.model).finish()
    }
}

impl OllamaClient {
    pub fn new(config: OllamaConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs_f64(config.timeout_seconds))
            .build()
            .map_err(|e| HertaError::llm("ollama", e.to_string()))?;
        Ok(Self { config, http })
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.config.host.trim_end_matches('/'), path.trim_start_matches('/'))
    }

    fn options(&self) -> Value {
        let mut opts = json!({
            "temperature": self.config.temperature,
            "num_ctx": self.config.num_ctx,
        });
        if let Some(num_gpu) = self.config.num_gpu {
            opts["num_gpu"] = json!(num_gpu);
        }
        opts
    }

    fn messages_to_json(messages: &[Message]) -> Vec<Value> {
        messages.iter().map(|m| json!({ "role": m.role.as_str(), "content": m.content })).collect()
    }

    async fn post_chat(&self, body: Value) -> Result<Value> {
        with_backoff(5, || {
            let http = &self.http;
            let endpoint = self.endpoint("api/chat");
            let body = body.clone();
            async move {
                let resp = http
                    .post(&endpoint)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| (true, HertaError::Network(e.to_string())))?;
                let status = resp.status().as_u16();
                if status == 200 {
                    let value: Value = resp
                        .json()
                        .await
                        .map_err(|e| (false, HertaError::llm("ollama", e.to_string())))?;
                    return Ok(value);
                }
                let retryable = is_retryable_status(status);
                let text = resp.text().await.unwrap_or_default();
                Err((retryable, HertaError::llm("ollama", format!("HTTP {status}: {text}"))))
            }
        })
        .await
    }

    fn extract_text(value: &Value) -> String {
        value.pointer("/message/content").and_then(Value::as_str).map(sanitize_reply).unwrap_or_default()
    }

    fn extract_tool_calls(value: &Value) -> Vec<ToolCall> {
        let Some(calls) = value.pointer("/message/tool_calls").and_then(Value::as_array) else {
            return Vec::new();
        };
        calls
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let name = c.pointer("/function/name").and_then(Value::as_str)?.to_string();
                // Ollama отдаёт аргументы уже как объект, а не как строку.
                let arguments = c.pointer("/function/arguments").cloned().unwrap_or_else(|| json!({}));
                Some(ToolCall { id: format!("call_{i}"), name, arguments })
            })
            .collect()
    }
}

#[async_trait]
impl ChatClient for OllamaClient {
    fn provider_name(&self) -> &'static str {
        "ollama"
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    async fn warm_up(&self) -> Result<()> {
        // Лёгкий чат с пустым промптом загружает модель в память.
        let body = json!({
            "model": self.config.model,
            "messages": [{ "role": "user", "content": "" }],
            "stream": false,
            "keep_alive": self.config.keep_alive,
            "options": self.options(),
        });
        self.post_chat(body).await.map(|_| ())
    }

    async fn chat(&self, messages: &[Message]) -> Result<String> {
        let body = json!({
            "model": self.config.model,
            "messages": Self::messages_to_json(messages),
            "stream": false,
            "think": self.config.think,
            "keep_alive": self.config.keep_alive,
            "options": self.options(),
        });
        let value = self.post_chat(body).await?;
        Ok(Self::extract_text(&value))
    }

    async fn chat_with_tools(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<ChatResponse> {
        if tools.is_empty() {
            return Ok(ChatResponse::text(self.chat(messages).await?));
        }
        let tool_schemas: Vec<Value> = tools.iter().map(ToolSpec::to_json_schema).collect();
        let body = json!({
            "model": self.config.model,
            "messages": Self::messages_to_json(messages),
            "stream": false,
            "think": self.config.think,
            "keep_alive": self.config.keep_alive,
            "tools": tool_schemas,
            "options": self.options(),
        });
        let value = self.post_chat(body).await?;
        Ok(ChatResponse {
            text: Self::extract_text(&value),
            tool_calls: Self::extract_tool_calls(&value),
        })
    }
}
