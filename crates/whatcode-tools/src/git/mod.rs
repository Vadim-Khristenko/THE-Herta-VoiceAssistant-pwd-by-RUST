//! Интеграция с Git: чтение, запись и полуавтоматические workflows.
//!
//! Модуль разделён на три слоя:
//! - [`read`] — только чтение: status, log, diff, branches, show, grep.
//! - [`write`] — мутации: add, commit, push, pull, checkout, stash, reset, revert, rebase, cherry-pick.
//! - [`advanced`] — полуавтоматические сценарии: откат коммита, синхронизация ветки и т.д.
//!
//! Все пути проверяются на `..` и аргументы, начинающиеся с `-`, чтобы
//! исключить простейшие инъекции. Деструктивные операции помечаются
//! соответствующим `ToolRisk` и в режиме `auto` требуют `/allow`.

pub mod advanced;
pub mod read;
pub mod write;

pub use advanced::*;
pub use read::*;
pub use write::*;

use crate::util::run_capture;
use whatcode_core::ToolResult;
use std::path::PathBuf;

const TIMEOUT_SECS: u64 = 30;
const LONG_TIMEOUT_SECS: u64 = 120;

/// Общий контекст для Git-инструментов.
#[derive(Clone)]
pub struct GitContext {
    pub repo_root: PathBuf,
}

impl GitContext {
    pub fn new() -> Self {
        Self {
            repo_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    /// Выполнить git-подкоманду с аргументами.
    pub async fn git(&self, tool: &'static str, args: &[&str]) -> ToolResult {
        self.git_with_timeout(tool, args, TIMEOUT_SECS).await
    }

    /// Для команд, которые могут занимать больше времени (push/pull/rebase).
    pub async fn git_long(&self, tool: &'static str, args: &[&str]) -> ToolResult {
        self.git_with_timeout(tool, args, LONG_TIMEOUT_SECS).await
    }

    /// Выполнить git с явным таймаутом.
    async fn git_with_timeout(
        &self,
        tool: &'static str,
        args: &[&str],
        timeout_secs: u64,
    ) -> ToolResult {
        match run_capture("git", args, Some(&self.repo_root), timeout_secs).await {
            Ok(out) if out.combined.is_empty() => ToolResult::ok(tool, "(пусто)"),
            Ok(out) => ToolResult::ok(tool, out.combined),
            Err(e) => ToolResult::rejected(tool, e),
        }
    }

    /// Выполнить git-команду с одним дополнительным проверенным аргументом.
    /// Если аргумент не проходит валидацию, возвращает `ToolResult::rejected`.
    pub async fn git_safe_arg(
        &self,
        tool: &'static str,
        args: &[&str],
        extra: &str,
    ) -> ToolResult {
        if !Self::safe_arg(extra) {
            return ToolResult::rejected(tool, "аргумент не должен содержать `..` или начинаться с `-`");
        }
        let mut full = args.to_vec();
        full.push(extra);
        self.git(tool, &full).await
    }

    /// Проверить, что строка безопасна для передачи как аргумент git.
    pub fn safe_arg(arg: &str) -> bool {
        !arg.contains("..") && !arg.starts_with('-') && !arg.is_empty()
    }
}

impl Default for GitContext {
    fn default() -> Self {
        Self::new()
    }
}
