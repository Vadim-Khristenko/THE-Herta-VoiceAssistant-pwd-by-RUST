//! Инструменты статического анализа: `type_check` (mypy) и `lint_code` (ruff).
//! Только чтение. Цель валидируется относительно корня проекта.

use crate::registry::Tool;
use crate::safety::path_within_root;
use async_trait::async_trait;
use herta_core::config::CodeToolsConfig;
use herta_core::{ParamType, ToolCall, ToolParameter, ToolResult, ToolSpec};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

const MAX_OUTPUT_CHARS: usize = 4000;

fn truncate(mut s: String) -> String {
    if s.chars().count() > MAX_OUTPUT_CHARS {
        s = s.chars().take(MAX_OUTPUT_CHARS).collect::<String>();
        s.push_str("\n… (вывод обрезан)");
    }
    s
}

/// Базовая логика запуска внешнего анализатора.
struct Analyzer {
    config: CodeToolsConfig,
    program: &'static str,
    base_args: Vec<&'static str>,
}

impl Analyzer {
    fn resolve_target(&self, target: &str) -> Option<PathBuf> {
        let root = PathBuf::from(&self.config.project_root);
        let candidate = root.join(target);
        if path_within_root(&root, &candidate) {
            Some(candidate)
        } else {
            None
        }
    }

    async fn run(&self, tool_name: &'static str, target: &str) -> ToolResult {
        if !self.config.enabled {
            return ToolResult::rejected(tool_name, "инструменты кода отключены в конфигурации");
        }
        let Some(path) = self.resolve_target(target) else {
            return ToolResult::rejected(tool_name, "путь вне корня проекта запрещён");
        };

        let mut cmd = Command::new(self.program);
        cmd.args(&self.base_args).arg(&path);

        let fut = cmd.output();
        let timed = tokio::time::timeout(Duration::from_secs(self.config.timeout_seconds), fut).await;

        match timed {
            Err(_) => ToolResult::rejected(tool_name, "превышен таймаут анализа"),
            Ok(Err(e)) => ToolResult::rejected(tool_name, format!("не удалось запустить {}: {e}", self.program)),
            Ok(Ok(output)) => {
                let mut combined = String::new();
                combined.push_str(&String::from_utf8_lossy(&output.stdout));
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.trim().is_empty() {
                    combined.push_str(&stderr);
                }
                let clean = combined.trim().to_string();
                let message = if clean.is_empty() {
                    format!("{}: замечаний нет.", self.program)
                } else {
                    truncate(clean)
                };
                ToolResult::ok(tool_name, message)
            }
        }
    }
}

pub struct TypeCheckTool {
    analyzer: Analyzer,
}

impl TypeCheckTool {
    pub fn new(config: CodeToolsConfig) -> Self {
        Self {
            analyzer: Analyzer {
                config,
                program: "mypy",
                base_args: vec!["--no-color-output", "--show-error-codes", "--no-error-summary", "--ignore-missing-imports"],
            },
        }
    }
}

#[async_trait]
impl Tool for TypeCheckTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "type_check",
            "Проверить типы в файле/директории через mypy (относительно корня проекта).",
            vec![ToolParameter::new("target", ParamType::String, "Путь относительно корня", true)],
        )
    }

    async fn call(&self, call: &ToolCall) -> ToolResult {
        match call.arg_str("target") {
            Some(target) => self.analyzer.run("type_check", &target).await,
            None => ToolResult::rejected("type_check", "не передан `target`"),
        }
    }
}

pub struct LintTool {
    analyzer: Analyzer,
}

impl LintTool {
    pub fn new(config: CodeToolsConfig) -> Self {
        Self {
            analyzer: Analyzer {
                config,
                program: "ruff",
                base_args: vec!["check", "--no-cache", "--select=E,F,W,UP,B,SIM"],
            },
        }
    }
}

#[async_trait]
impl Tool for LintTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "lint_code",
            "Линтинг файла/директории через ruff (относительно корня проекта).",
            vec![ToolParameter::new("target", ParamType::String, "Путь относительно корня", true)],
        )
    }

    async fn call(&self, call: &ToolCall) -> ToolResult {
        match call.arg_str("target") {
            Some(target) => self.analyzer.run("lint_code", &target).await,
            None => ToolResult::rejected("lint_code", "не передан `target`"),
        }
    }
}
