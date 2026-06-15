//! `herta-llm` — абстракция провайдеров LLM.
//!
//! Единый async-трейт [`ChatClient`] поверх трёх семейств бэкендов:
//! локальный Ollama, OpenAI-совместимые облака (Cerebras, DeepSeek) и Google AI
//! (Gemini/Gemma). Слой выше не знает, какой бэкенд активен, — только трейт.

#![forbid(unsafe_code)]

pub mod google;
pub mod ollama;
pub mod openai_compat;
pub mod retry;

use async_trait::async_trait;
use herta_core::{AppConfig, HertaError, LlmProvider, Message, Result, ToolCall, ToolSpec};

/// Ответ модели: текст и (опционально) запрошенные вызовы инструментов.
#[derive(Debug, Clone, Default)]
pub struct ChatResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
}

impl ChatResponse {
    pub fn text(text: impl Into<String>) -> Self {
        Self { text: text.into(), tool_calls: Vec::new() }
    }

    pub fn wants_tools(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

/// Общий интерфейс провайдера диалога. Объектно-безопасен (`dyn ChatClient`).
#[async_trait]
pub trait ChatClient: Send + Sync {
    /// Человекочитаемое имя провайдера (для логов и ошибок).
    fn provider_name(&self) -> &'static str;

    /// Имя активной модели.
    fn model_name(&self) -> &str;

    /// Прогрев/проверка доступности. По умолчанию - успех.
    async fn warm_up(&self) -> Result<()> {
        Ok(())
    }

    /// Простой чат без инструментов.
    async fn chat(&self, messages: &[Message]) -> Result<String>;

    /// Чат с инструментами. По умолчанию инструменты игнорируются и
    /// возвращается обычный текстовый ответ — провайдеры без нативного
    /// function-calling используют это поведение осознанно, а не как заглушку.
    async fn chat_with_tools(&self, messages: &[Message], _tools: &[ToolSpec]) -> Result<ChatResponse> {
        Ok(ChatResponse::text(self.chat(messages).await?))
    }
}

/// Сборка клиента по активному провайдеру из конфигурации.
pub fn build_client(config: &AppConfig) -> Result<Box<dyn ChatClient>> {
    match config.llm_provider {
        LlmProvider::Ollama => Ok(Box::new(ollama::OllamaClient::new(config.ollama.clone())?)),
        LlmProvider::Cerebras => {
            let c = config.cerebras.clone();
            ensure_key(&c.api_key, "cerebras")?;
            Ok(Box::new(openai_compat::OpenAiCompatClient::new("cerebras", c)?))
        }
        LlmProvider::DeepSeek => {
            let c = config.deepseek.clone();
            ensure_key(&c.api_key, "deepseek")?;
            Ok(Box::new(openai_compat::OpenAiCompatClient::new("deepseek", c)?))
        }
        LlmProvider::GoogleAi => {
            let c = config.google_ai.clone();
            if c.api_key.is_none() {
                return Err(HertaError::llm("google_ai", "не задан GOOGLE_AI_API_KEY"));
            }
            Ok(Box::new(google::GoogleAiClient::new(c)?))
        }
    }
}

fn ensure_key(key: &Option<String>, provider: &str) -> Result<()> {
    if key.is_none() {
        return Err(HertaError::llm(provider.to_string(), "не задан API-ключ"));
    }
    Ok(())
}

/// Удалить блоки рассуждений `<think>...</think>` и обрезать пробелы.
/// Персона Герты запрещает вывод внутренних черновиков, поэтому чистим на
/// уровне провайдера, не полагаясь на дисциплину модели.
pub fn sanitize_reply(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end_rel) => {
                let end = start + end_rel + "</think>".len();
                rest = &rest[end..];
            }
            // Незакрытый блок — отбрасываем всё до конца.
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::sanitize_reply;

    #[test]
    fn strips_think_blocks() {
        assert_eq!(sanitize_reply("<think>план</think>Ответ."), "Ответ.");
        assert_eq!(sanitize_reply("Текст <think>шум</think> ещё."), "Текст  ещё.");
        assert_eq!(sanitize_reply("Хвост <think>обрыв"), "Хвост");
        assert_eq!(sanitize_reply("  Чисто.  "), "Чисто.");
    }
}
