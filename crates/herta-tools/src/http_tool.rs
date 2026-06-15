//! Сетевой инструмент `fetch_url`: загрузить содержимое HTTP(S)-страницы.
//! В отличие от `web_search`, это прямое получение конкретного URL.

use crate::registry::Tool;
use crate::util::truncate;
use async_trait::async_trait;
use herta_core::{ParamType, ToolCall, ToolParameter, ToolResult, ToolSpec};
use std::time::Duration;

pub struct FetchUrlTool {
    http: reqwest::Client,
}

impl Default for FetchUrlTool {
    fn default() -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent("TheHerta/0.4 (+https://vai-rice.space)")
            .build()
            .unwrap_or_default();
        Self { http }
    }
}

#[async_trait]
impl Tool for FetchUrlTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "fetch_url",
            "Загрузить содержимое конкретного HTTP(S)-адреса (GET) и вернуть его как текст. \
             Разрешены только схемы http и https. Бинарные ответы и слишком большой текст усекаются. \
             Используй, когда пользователь дал точный URL и нужно прочитать именно его (в отличие от \
             поискового запроса через web_search).",
            vec![ToolParameter::new("url", ParamType::String, "Полный адрес, начинающийся с http:// или https://", true)],
        )
    }

    async fn call(&self, call: &ToolCall) -> ToolResult {
        let Some(url) = call.arg_str("url") else {
            return ToolResult::rejected("fetch_url", "не передан `url`");
        };
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return ToolResult::rejected("fetch_url", "разрешены только http(s)-адреса");
        }
        match self.http.get(&url).send().await {
            Ok(resp) => {
                let status = resp.status();
                match resp.text().await {
                    Ok(body) => {
                        let header = format!("HTTP {} | {}\n\n", status.as_u16(), url);
                        ToolResult::ok("fetch_url", truncate(format!("{header}{body}")))
                    }
                    Err(e) => {
                        ToolResult::rejected("fetch_url", format!("тело ответа не текстовое: {e}"))
                    }
                }
            }
            Err(e) => ToolResult::rejected("fetch_url", format!("сеть: {e}")),
        }
    }
}
