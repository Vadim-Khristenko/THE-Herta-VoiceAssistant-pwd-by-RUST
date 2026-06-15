//! Сборка реестра инструментов из конфигурации.
//!
//! Базовый набор (git, файлы, время, fetch_url, навыки) доступен всегда;
//! память, веб-поиск, анализ кода и системные действия включаются по флагам.

use crate::code_tools::{LintTool, TypeCheckTool};
use crate::fs_tools::{ListDirTool, ReadFileTool};
use crate::git::{GitBranchTool, GitDiffTool, GitLogTool, GitStatusTool};
use crate::http_tool::FetchUrlTool;
use crate::memory_tools::{ForgetTool, RecallTool, RememberTool};
use crate::registry::ToolRegistry;
use crate::skills::{ListSkillsTool, SkillLibrary, UseSkillTool};
use crate::system_actions::{CreateNoteTool, OpenUrlTool};
use crate::time_tool::CurrentTimeTool;
use crate::web_search::WebSearchTool;
use herta_core::config::AppConfig;
use herta_core::LongMemoryStore;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Каталог навыков из окружения `HERTA_SKILLS_DIR` (по умолчанию `skills`).
fn skills_dir() -> String {
    std::env::var("HERTA_SKILLS_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "skills".to_string())
}

/// Построить полный реестр инструментов для агента.
pub fn build_registry(
    config: &AppConfig,
    long_memory: Arc<Mutex<LongMemoryStore>>,
) -> ToolRegistry {
    let mut reg = ToolRegistry::new();

    // --- базовый набор: всегда доступен ---
    reg.register(Arc::new(GitStatusTool::default()))
        .register(Arc::new(GitLogTool::default()))
        .register(Arc::new(GitDiffTool::default()))
        .register(Arc::new(GitBranchTool::default()))
        .register(Arc::new(ReadFileTool))
        .register(Arc::new(ListDirTool))
        .register(Arc::new(FetchUrlTool::default()))
        .register(Arc::new(CurrentTimeTool));

    // --- навыки (прогрессивное раскрытие), если каталог не пуст ---
    let library = SkillLibrary::load(skills_dir());
    if !library.is_empty() {
        reg.register(Arc::new(ListSkillsTool::new(library.clone())))
            .register(Arc::new(UseSkillTool::new(library)));
    }

    // --- долговременная память ---
    if config.long_memory.enabled {
        reg.register(Arc::new(RememberTool::new(Arc::clone(&long_memory))))
            .register(Arc::new(RecallTool::new(Arc::clone(&long_memory))))
            .register(Arc::new(ForgetTool::new(Arc::clone(&long_memory))));
    }

    // --- веб-поиск ---
    if config.web_search.enabled {
        reg.register(Arc::new(WebSearchTool::new(config.web_search.clone())));
    }

    // --- анализ кода ---
    if config.code_tools.enabled {
        reg.register(Arc::new(TypeCheckTool::new(config.code_tools.clone())))
            .register(Arc::new(LintTool::new(config.code_tools.clone())));
    }

    // --- системные действия ---
    if config.system_actions.enabled {
        reg.register(Arc::new(OpenUrlTool::new(config.system_actions.clone())))
            .register(Arc::new(CreateNoteTool::new(config.system_actions.clone())));
    }

    reg
}
