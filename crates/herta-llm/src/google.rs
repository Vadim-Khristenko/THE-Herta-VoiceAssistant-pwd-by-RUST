//! Клиент Google AI Studio (Gemini / Gemma) через REST `generateContent`.
//!
//! Протокол отличается от OpenAI: роли `user`/`model`, системная инструкция
//! отдельным полем, контент в виде массива `parts`.

use crate::retry::{is_retryable_status, with_backoff};
use crate::{sanitize_reply, ChatClient};
use async_trait::async_trait;
use herta_core::config::GoogleAiConfig;
use herta_core::{HertaError, Message, Result, Role};
use serde_json::{json, Value};
use std::time::Duration;

pub struct GoogleAiClient {
    config: GoogleAiConfig,
    http: reqwest::Client,
}

impl std::fmt::Debug for GoogleAiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleAiClient").field("model", &self.config.model).finish()
    }
}

impl GoogleAiClient {
    pub fn new(config: GoogleAiConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs_f64(config.timeout_seconds))
            .build()
            .map_err(|e| HertaError::llm("google_ai", e.to_string()))?;
        Ok(Self { config, http })
    }

    /// Разделяет историю на системную инструкцию и контент в формате Gemini.
    fn split_payload(messages: &[Message]) -> (Option<String>, Vec<Value>) {
        let mut system_parts: Vec<String> = Vec::new();
        let mut contents: Vec<Value> = Vec::new();
        for m in messages {
            match m.role {
                Role::System => system_parts.push(m.content.clone()),
                Role::User | Role::Tool => {
                    contents.push(json!({ "role": "user", "parts": [{ "text": m.content }] }));
                }
                Role::Assistant => {
                    contents.push(json!({ "role": "model", "parts": [{ "text": m.content }] }));
                }
            }
        }
        let system = if system_parts.is_empty() { None } else { Some(system_parts.join("\n\n")) };
        (system, contents)
    }

    fn endpoint(&self, model: &str, key: &str) -> String {
        format!(
            "{}/models/{}:generateContent?key={}",
            self.config.base_url.trim_end_matches('/'),
            model,
            key
        )
    }

    async fn generate(&self, model: &str, messages: &[Message]) -> Result<String> {
        let key = self
            .config
            .api_key
            .clone()
            .ok_or_else(|| HertaError::llm("google_ai", "не задан API-ключ"))?;
        let (system, contents) = Self::split_payload(messages);

        let mut body = json!({
            "contents": contents,
            "generationConfig": {
                "temperature": self.config.temperature,
                "maxOutputTokens": self.config.max_tokens,
            }
        });
        if let Some(sys) = system {
            body["systemInstruction"] = json!({ "parts": [{ "text": sys }] });
        }

        let value = with_backoff(3, || {
            let http = &self.http;
            let endpoint = self.endpoint(model, &key);
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
                    let v: Value = resp
                        .json()
                        .await
                        .map_err(|e| (false, HertaError::llm("google_ai", e.to_string())))?;
                    return Ok(v);
                }
                let retryable = is_retryable_status(status);
                let text = resp.text().await.unwrap_or_default();
                Err((retryable, HertaError::llm("google_ai", format!("HTTP {status}: {text}"))))
            }
        })
        .await?;

        Ok(Self::extract_text(&value))
    }

    fn extract_text(value: &Value) -> String {
        let Some(parts) = value.pointer("/candidates/0/content/parts").and_then(Value::as_array) else {
            return String::new();
        };
        let joined: String = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("");
        sanitize_reply(&joined)
    }
}

#[async_trait]
impl ChatClient for GoogleAiClient {
    fn provider_name(&self) -> &'static str {
        "google_ai"
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    async fn warm_up(&self) -> Result<()> {
        if self.config.api_key.is_none() {
            return Err(HertaError::llm("google_ai", "не задан API-ключ"));
        }
        Ok(())
    }

    async fn chat(&self, messages: &[Message]) -> Result<String> {
        match self.generate(&self.config.model, messages).await {
            Ok(text) => Ok(text),
            Err(primary) => {
                // Падение основной модели — пробуем резервную, если задана.
                if let Some(fallback) = &self.config.fallback_model {
                    tracing::warn!(error = %primary, fallback, "google_ai: переключаюсь на резервную модель");
                    return self.generate(fallback, messages).await;
                }
                Err(primary)
            }
        }
    }

    // chat_with_tools использует поведение по умолчанию из трейта: Gemma-модели
    // в этом REST-режиме не получают function-calling, поэтому отдаём текст.
}

#[cfg(test)]
mod tests {
    use super::*;
    use herta_core::Message;

    #[test]
    fn split_separates_system_and_roles() {
        let msgs = vec![
            Message::system("персона"),
            Message::user("привет"),
            Message::assistant("Уже лучше."),
        ];
        let (system, contents) = GoogleAiClient::split_payload(&msgs);
        assert_eq!(system.as_deref(), Some("персона"));
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[1]["role"], "model");
    }
}
