//! Интеграция с Git (только чтение). Инструменты дают модели обзор репозитория
//! без мутаций: статус, история, дифф, текущая ветка, список веток.
//!
//! Все команды выполняются в каталоге `repo_root` с таймаутом. Деструктивные
//! операции (commit/push/reset) сознательно не предоставляются — это политика
//! безопасности уровня инструмента.

use crate::registry::Tool;
use crate::util::run_capture;
use async_trait::async_trait;
use herta_core::{ParamType, ToolCall, ToolParameter, ToolResult, ToolSpec};
use std::path::PathBuf;

const TIMEOUT_SECS: u64 = 15;

#[derive(Clone)]
struct GitContext {
    repo_root: PathBuf,
}

impl GitContext {
    fn new() -> Self {
        Self {
            repo_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    async fn git(&self, tool: &'static str, args: &[&str]) -> ToolResult {
        match run_capture("git", args, Some(&self.repo_root), TIMEOUT_SECS).await {
            Ok(out) if out.combined.is_empty() => ToolResult::ok(tool, "(пусто)"),
            Ok(out) => ToolResult::ok(tool, out.combined),
            Err(e) => ToolResult::rejected(tool, e),
        }
    }
}

/// `git_status` — рабочее дерево в кратком формате.
pub struct GitStatusTool {
    ctx: GitContext,
}
impl Default for GitStatusTool {
    fn default() -> Self {
        Self {
            ctx: GitContext::new(),
        }
    }
}

#[async_trait]
impl Tool for GitStatusTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "git_status",
            "Показать состояние рабочего дерева Git (изменённые, добавленные, неотслеживаемые файлы) \
             в кратком формате `git status --short`. Только чтение. Используй, чтобы понять, что \
             сейчас изменено в репозитории, прежде чем советовать действия.",
            vec![],
        )
    }
    async fn call(&self, _call: &ToolCall) -> ToolResult {
        self.ctx
            .git("git_status", &["status", "--short", "--branch"])
            .await
    }
}

/// `git_log` — последние коммиты.
pub struct GitLogTool {
    ctx: GitContext,
}
impl Default for GitLogTool {
    fn default() -> Self {
        Self {
            ctx: GitContext::new(),
        }
    }
}

#[async_trait]
impl Tool for GitLogTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "git_log",
            "Показать последние коммиты Git одной строкой каждый (хеш, автор, относительная дата, \
             заголовок). Только чтение. Параметр `count` ограничивает число коммитов (по умолчанию 10, \
             максимум 50).",
            vec![ToolParameter::new("count", ParamType::Integer, "Сколько коммитов показать (1..50)", false)],
        )
    }
    async fn call(&self, call: &ToolCall) -> ToolResult {
        let count = call
            .arguments
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .clamp(1, 50);
        let fmt = "--pretty=format:%h %an %ar %s";
        let n = format!("-n{count}");
        self.ctx.git("git_log", &["log", &n, fmt]).await
    }
}

/// `git_diff` — несохранённые изменения (опционально по одному пути).
pub struct GitDiffTool {
    ctx: GitContext,
}
impl Default for GitDiffTool {
    fn default() -> Self {
        Self {
            ctx: GitContext::new(),
        }
    }
}

#[async_trait]
impl Tool for GitDiffTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "git_diff",
            "Показать несохранённые изменения рабочего дерева (`git diff`). Только чтение. \
             Необязательный `path` ограничивает дифф одним файлом или каталогом относительно корня \
             репозитория. Используй для ревью правок перед коммитом.",
            vec![ToolParameter::new(
                "path",
                ParamType::String,
                "Путь относительно корня репозитория",
                false,
            )],
        )
    }
    async fn call(&self, call: &ToolCall) -> ToolResult {
        match call.arg_str("path") {
            Some(path) => {
                // Запрещаем выход за пределы репозитория простым правилом.
                if path.contains("..") {
                    return ToolResult::rejected("git_diff", "путь не должен содержать `..`");
                }
                self.ctx.git("git_diff", &["diff", "--", &path]).await
            }
            None => self.ctx.git("git_diff", &["diff"]).await,
        }
    }
}

/// `git_branches` — текущая ветка и список локальных веток.
pub struct GitBranchTool {
    ctx: GitContext,
}
impl Default for GitBranchTool {
    fn default() -> Self {
        Self {
            ctx: GitContext::new(),
        }
    }
}

#[async_trait]
impl Tool for GitBranchTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "git_branches",
            "Показать локальные ветки Git; текущая помечена `*`. Только чтение. Используй, чтобы \
             узнать, на какой ветке идёт работа и какие ещё ветки есть.",
            vec![],
        )
    }
    async fn call(&self, _call: &ToolCall) -> ToolResult {
        self.ctx.git("git_branches", &["branch", "--list"]).await
    }
}
