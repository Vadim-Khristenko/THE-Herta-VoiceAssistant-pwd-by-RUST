//! Инструмент `current_time`: текущие дата и время (UTC, ISO-8601).

use crate::registry::Tool;
use async_trait::async_trait;
use herta_core::{ToolCall, ToolResult, ToolSpec};
use serde_json::json;

/// `current_time` — текущая дата/время в UTC.
#[derive(Default)]
pub struct CurrentTimeTool;

#[async_trait]
impl Tool for CurrentTimeTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "current_time",
            "Вернуть текущие дату и время в UTC в формате ISO-8601. Без параметров. Используй, когда \
             нужна актуальная дата/время — например, чтобы рассчитать относительные сроки или ответить \
             «какое сегодня число».",
            vec![],
        )
    }

    async fn call(&self, _call: &ToolCall) -> ToolResult {
        let now = chrono::Utc::now();
        let iso = now.to_rfc3339();
        ToolResult::ok("current_time", format!("Сейчас (UTC): {iso}"))
            .with_data(json!({ "iso8601": iso, "unix": now.timestamp() }))
    }
}
