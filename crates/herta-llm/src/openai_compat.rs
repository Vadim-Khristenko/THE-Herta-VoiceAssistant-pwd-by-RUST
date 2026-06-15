//! OpenAI-совместимый клиент. Покрывает Cerebras и DeepSeek: одинаковый
//! протокол `/chat/completions`, различаются только `base_url` и модель.

use crate::retry::{is_retryable_status, with_backoff};
use crate::{sanitize_reply, ChatClient, ChatResponse};
use async_trait::async_trait;
use herta_core::config::OpenAiCompatConfig;
use herta_core::{HertaError, Message, Result, ToolCall, ToolSpec};
use serde_json::{json, Value};
use std::time::Duration;

pub struct OpenAiCompatClient {
    provider: &'static str,
    config: OpenAiCompatConfig,
    http: reqwest::Client,
}

impl std::fmt::Debug for OpenAiCompatClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatClient").field("provider", &self.provider).field("model", &self.config.model).finish()
    }
}

impl OpenAiCompatClient {
    pub fn new(provider: &'static str, config: OpenAiCompatConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs_f64(config.timeout_seconds))
            .build()
            .map_err(|e| HertaError::llm(provider, e.to_string()))?;
        Ok(Self { provider, config, http })
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'))
    }

    fn messages_to_json(messages: &[Message]) -> Vec<Value> {
        messages
            .iter()
            .map(|m| {
                let mut obj = json!({ "role": m.role.as_str(), "content": m.content });
                if let Some(id) = &m.tool_call_id {
                    obj["tool_call_id"] = json!(id);
                }
                obj
            })
            .collect()
    }

    async fn post(&self, body: Value) -> Result<Value> {
        let key = self
            .config
            .api_key
            .clone()
            .ok_or_else(|| HertaError::llm(self.provider, "отсутствует API-ключ"))?;

        with_backoff(self.config.retry_attempts + 1, || {
            let http = &self.http;
            let endpoint = self.endpoint();
            let key = key.clone();
            let body = body.clone();
            let provider = self.provider;
            async move {
                let resp = http
                    .post(&endpoint)
                    .bearer_auth(&key)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| (true, HertaError::Network(e.to_string())))?;

                let status = resp.status().as_u16();
                if status == 200 {
                    let value: Value = resp
                        .json()
                        .await
                        .map_err(|e| (false, HertaError::llm(provider, e.to_string())))?;
                    return Ok(value);
                }
                let retryable = is_retryable_status(status);
                let text = resp.text().await.unwrap_or_default();
                Err((retryable, HertaError::llm(provider, format!("HTTP {status}: {text}"))))
            }
        })
        .await
    }

    fn extract_text(value: &Value) -> String {
        value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(sanitize_reply)
            .unwrap_or_default()
    }

    fn extract_tool_calls(value: &Value) -> Vec<ToolCall> {
        let Some(calls) = value.pointer("/choices/0/message/tool_calls").and_then(Value::as_array) else {
            return Vec::new();
        };
        calls
            .iter()
            .filter_map(|c| {
                let id = c.get("id").and_then(Value::as_str).unwrap_or("call").to_string();
                let name = c.pointer("/function/name").and_then(Value::as_str)?.to_string();
                let args_raw = c.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("{}");
                let arguments = serde_json::from_str::<Value>(args_raw).unwrap_or_else(|_| json!({}));
                Some(ToolCall { id, name, arguments })
            })
            .collect()
    }
}

#[async_trait]
impl ChatClient for OpenAiCompatClient {
    fn provider_name(&self) -> &'static str {
        self.provider
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    async fn warm_up(&self) -> Result<()> {
        if self.config.api_key.is_none() {
            return Err(HertaError::llm(self.provider, "не задан API-ключ"));
        }
        Ok(())
    }

    async fn chat(&self, messages: &[Message]) -> Result<String> {
        let body = json!({
            "model": self.config.model,
            "messages": Self::messages_to_json(messages),
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
            "stream": false,
        });
        let value = self.post(body).await?;
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
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
            "tools": tool_schemas,
            "tool_choice": "auto",
            "stream": false,
        });
        let value = self.post(body).await?;
        Ok(ChatResponse {
            text: Self::extract_text(&value),
            tool_calls: Self::extract_tool_calls(&value),
        })
    }
}
