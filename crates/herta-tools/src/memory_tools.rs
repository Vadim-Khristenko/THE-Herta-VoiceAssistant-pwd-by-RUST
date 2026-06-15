//! Инструменты долговременной памяти: `remember`, `recall`, `forget`.
//! Делят один `LongMemoryStore` под асинхронным мьютексом.

use crate::registry::Tool;
use async_trait::async_trait;
use herta_core::{FactCategory, FactSource, LongMemoryStore, ParamType, ToolCall, ToolParameter, ToolResult, ToolSpec};
use std::sync::Arc;
use tokio::sync::Mutex;

type Store = Arc<Mutex<LongMemoryStore>>;

/// `remember(content, category)` — сохранить факт.
pub struct RememberTool {
    store: Store,
}

impl RememberTool {
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for RememberTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "remember",
            "Сохранить стабильный факт о пользователе, проекте или предпочтениях в долговременную память.",
            vec![
                ToolParameter::new("content", ParamType::String, "Текст факта", true),
                ToolParameter::new("category", ParamType::String, "user | project | preferences | notes", false),
            ],
        )
    }

    async fn call(&self, call: &ToolCall) -> ToolResult {
        let Some(content) = call.arg_str("content") else {
            return ToolResult::rejected("remember", "не передан `content`");
        };
        let category = call.arg_str("category").map(|c| FactCategory::parse(&c)).unwrap_or(FactCategory::Notes);
        let mut store = self.store.lock().await;
        match store.add_fact(&content, category, FactSource::Explicit) {
            Ok(Some(fact)) => ToolResult::ok("remember", format!("Запомнила: {}", fact.content)),
            Ok(None) => ToolResult::ok("remember", "Этот факт мне уже известен."),
            Err(e) => ToolResult::rejected("remember", e.to_string()),
        }
    }
}

/// `recall(category?)` — перечислить факты.
pub struct RecallTool {
    store: Store,
}

impl RecallTool {
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for RecallTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "recall",
            "Вспомнить сохранённые факты, опционально по категории.",
            vec![ToolParameter::new("category", ParamType::String, "user | project | preferences | notes", false)],
        )
    }

    async fn call(&self, call: &ToolCall) -> ToolResult {
        let store = self.store.lock().await;
        let listing = match call.arg_str("category") {
            Some(cat) => {
                let category = FactCategory::parse(&cat);
                store.by_category(category).iter().map(|f| format!("- {}", f.content)).collect::<Vec<_>>().join("\n")
            }
            None => store.all_facts().iter().map(|f| format!("- {}", f.content)).collect::<Vec<_>>().join("\n"),
        };
        if listing.is_empty() {
            ToolResult::ok("recall", "Пока ничего не сохранено.")
        } else {
            ToolResult::ok("recall", listing)
        }
    }
}

/// `forget(content_match)` — удалить факты по подстроке.
pub struct ForgetTool {
    store: Store,
}

impl ForgetTool {
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for ForgetTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "forget",
            "Удалить из памяти факты, содержащие указанную подстроку.",
            vec![ToolParameter::new("content_match", ParamType::String, "Подстрока для поиска", true)],
        )
    }

    async fn call(&self, call: &ToolCall) -> ToolResult {
        let Some(needle) = call.arg_str("content_match") else {
            return ToolResult::rejected("forget", "не передан `content_match`");
        };
        let mut store = self.store.lock().await;
        match store.remove_by_content(&needle) {
            Ok(0) => ToolResult::ok("forget", "Совпадений не найдено."),
            Ok(n) => ToolResult::ok("forget", format!("Удалено фактов: {n}")),
            Err(e) => ToolResult::rejected("forget", e.to_string()),
        }
    }
}
