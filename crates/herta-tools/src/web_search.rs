//! Веб-поиск через Tavily API. Инструмент `web_search(query)`.

use crate::registry::Tool;
use async_trait::async_trait;
use herta_core::config::WebSearchConfig;
use herta_core::{ParamType, ToolCall, ToolParameter, ToolResult, ToolSpec};
use serde_json::{json, Value};
use std::time::Duration;

pub struct WebSearchTool {
    config: WebSearchConfig,
    http: reqwest::Client,
}

impl WebSearchTool {
    pub fn new(config: WebSearchConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs_f64(config.timeout_seconds))
            .build()
            .unwrap_or_default();
        Self { config, http }
    }

    fn format_results(value: &Value, max: usize) -> String {
        let mut out = String::new();
        if let Some(answer) = value.get("answer").and_then(Value::as_str) {
            if !answer.trim().is_empty() {
                out.push_str("Сводка: ");
                out.push_str(answer.trim());
                out.push('\n');
            }
        }
        if let Some(results) = value.get("results").and_then(Value::as_array) {
            for (i, r) in results.iter().take(max).enumerate() {
                let title = r.get("title").and_then(Value::as_str).unwrap_or("(без названия)");
                let url = r.get("url").and_then(Value::as_str).unwrap_or("");
                out.push_str(&format!("{}. {} — {}\n", i + 1, title, url));
            }
        }
        if out.is_empty() {
            out.push_str("Поиск не дал результатов.");
        }
        out.trim_end().to_string()
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "web_search",
            "Поиск актуальной информации в интернете (новости, погода, факты, релизы).",
            vec![ToolParameter::new("query", ParamType::String, "Поисковый запрос", true)],
        )
    }

    async fn call(&self, call: &ToolCall) -> ToolResult {
        if !self.config.enabled {
            return ToolResult::rejected("web_search", "веб-поиск отключён в конфигурации");
        }
        let Some(api_key) = self.config.api_key.clone() else {
            return ToolResult::rejected("web_search", "не задан TAVILY_API_KEY");
        };
        let Some(query) = call.arg_str("query") else {
            return ToolResult::rejected("web_search", "не передан `query`");
        };

        let body = json!({
            "api_key": api_key,
            "query": query,
            "max_results": self.config.max_results,
            "search_depth": self.config.search_depth,
            "include_answer": true,
        });

        let resp = self.http.post("https://api.tavily.com/search").json(&body).send().await;
        match resp {
            Ok(r) if r.status().is_success() => match r.json::<Value>().await {
                Ok(value) => {
                    let formatted = Self::format_results(&value, self.config.max_results);
                    ToolResult::ok("web_search", formatted).with_data(value)
                }
                Err(e) => ToolResult::rejected("web_search", format!("разбор ответа: {e}")),
            },
            Ok(r) => ToolResult::rejected("web_search", format!("HTTP {}", r.status().as_u16())),
            Err(e) => ToolResult::rejected("web_search", format!("сеть: {e}")),
        }
    }
}
