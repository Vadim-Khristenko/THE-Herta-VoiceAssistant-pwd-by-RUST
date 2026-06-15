//! Реестр инструментов: хранение, схемы для модели, безопасная диспетчеризация.

use async_trait::async_trait;
use herta_core::{ToolCall, ToolResult, ToolSpec};
use std::collections::HashMap;
use std::sync::Arc;

/// Один исполняемый инструмент.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Схема инструмента для модели.
    fn spec(&self) -> ToolSpec;

    /// Выполнить вызов. Реализация не должна паниковать — только `ToolResult`.
    async fn call(&self, call: &ToolCall) -> ToolResult;
}

/// Набор зарегистрированных инструментов.
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// Разрешать ли деструктивные инструменты (по умолчанию — нет).
    allow_destructive: bool,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("tools", &self.tools.keys().collect::<Vec<_>>())
            .field("allow_destructive", &self.allow_destructive)
            .finish()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_destructive(mut self, allow: bool) -> Self {
        self.allow_destructive = allow;
        self
    }

    /// Зарегистрировать инструмент. Дубликаты по имени перезаписываются.
    pub fn register(&mut self, tool: Arc<dyn Tool>) -> &mut Self {
        self.tools.insert(tool.spec().name, tool);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Схемы всех инструментов для передачи модели.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|t| t.spec()).collect()
    }

    /// Выполнить вызов по имени. Неизвестное имя или деструктивный инструмент
    /// (когда они запрещены) возвращают `ToolResult` с `executed = false`.
    pub async fn dispatch(&self, call: &ToolCall) -> ToolResult {
        let Some(tool) = self.tools.get(&call.name) else {
            return ToolResult::rejected(call.name.clone(), format!("инструмент `{}` не найден", call.name));
        };
        let spec = tool.spec();
        if spec.destructive && !self.allow_destructive {
            return ToolResult::rejected(
                call.name.clone(),
                "деструктивное действие заблокировано политикой безопасности",
            );
        }
        tool.call(call).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use herta_core::{ParamType, ToolParameter};
    use serde_json::json;

    struct Echo;

    #[async_trait]
    impl Tool for Echo {
        fn spec(&self) -> ToolSpec {
            ToolSpec::new(
                "echo",
                "Вернуть переданный текст",
                vec![ToolParameter::new("text", ParamType::String, "что вернуть", true)],
            )
        }
        async fn call(&self, call: &ToolCall) -> ToolResult {
            ToolResult::ok("echo", call.arg_str("text").unwrap_or_default())
        }
    }

    struct Nuke;

    #[async_trait]
    impl Tool for Nuke {
        fn spec(&self) -> ToolSpec {
            ToolSpec::new("nuke", "опасное действие", vec![]).destructive()
        }
        async fn call(&self, _call: &ToolCall) -> ToolResult {
            ToolResult::ok("nuke", "выполнено")
        }
    }

    #[tokio::test]
    async fn dispatch_and_safety() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(Echo)).register(Arc::new(Nuke));

        let echo = reg.dispatch(&ToolCall { id: "1".into(), name: "echo".into(), arguments: json!({"text":"привет"}) }).await;
        assert!(echo.executed);
        assert_eq!(echo.message, "привет");

        let nuke = reg.dispatch(&ToolCall { id: "2".into(), name: "nuke".into(), arguments: json!({}) }).await;
        assert!(!nuke.executed);

        let missing = reg.dispatch(&ToolCall { id: "3".into(), name: "ghost".into(), arguments: json!({}) }).await;
        assert!(!missing.executed);
    }
}
