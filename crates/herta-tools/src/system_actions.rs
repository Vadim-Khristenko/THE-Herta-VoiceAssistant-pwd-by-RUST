//! Системные действия: открыть URL/приложение, создать текстовую заметку.
//! Деструктивные намерения блокируются; запись ограничена каталогом заметок.

use crate::registry::Tool;
use crate::safety::{looks_destructive, path_within_root};
use async_trait::async_trait;
use herta_core::config::SystemActionsConfig;
use herta_core::{ParamType, ToolCall, ToolParameter, ToolResult, ToolSpec};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

/// Команда открытия ресурса в зависимости от ОС.
fn open_command(target: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", target]);
        cmd
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(target);
        cmd
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(target);
        cmd
    }
}

fn documents_dir(cfg: &SystemActionsConfig) -> PathBuf {
    let dirs = directories_base();
    match cfg.document_dir.to_lowercase().as_str() {
        "desktop" => dirs.0,
        "documents" => dirs.1,
        custom => PathBuf::from(custom),
    }
}

// Каталоги рабочего стола и документов с разумными запасными вариантами.
fn directories_base() -> (PathBuf, PathBuf) {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    (home.join("Desktop"), home.join("Documents"))
}

/// `open_url(url)` — открыть ссылку в браузере по умолчанию.
pub struct OpenUrlTool {
    config: SystemActionsConfig,
}

impl OpenUrlTool {
    pub fn new(config: SystemActionsConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for OpenUrlTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "open_url",
            "Открыть URL в браузере по умолчанию.",
            vec![ToolParameter::new("url", ParamType::String, "Адрес для открытия", false)],
        )
    }

    async fn call(&self, call: &ToolCall) -> ToolResult {
        if !self.config.enabled {
            return ToolResult::rejected("open_url", "системные действия отключены");
        }
        let url = call.arg_str("url").unwrap_or_else(|| self.config.browser_home_url.clone());
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return ToolResult::rejected("open_url", "разрешены только http(s)-ссылки");
        }
        match open_command(&url).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(_) => ToolResult::ok("open_url", format!("Открываю: {url}")),
            Err(e) => ToolResult::rejected("open_url", format!("не удалось открыть: {e}")),
        }
    }
}

/// `create_note(name, content)` — создать .txt-заметку в каталоге документов.
pub struct CreateNoteTool {
    config: SystemActionsConfig,
}

impl CreateNoteTool {
    pub fn new(config: SystemActionsConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for CreateNoteTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "create_note",
            "Создать текстовую заметку (.txt) в каталоге документов пользователя.",
            vec![
                ToolParameter::new("name", ParamType::String, "Имя файла без расширения", true),
                ToolParameter::new("content", ParamType::String, "Содержимое заметки", false),
            ],
        )
    }

    async fn call(&self, call: &ToolCall) -> ToolResult {
        if !self.config.enabled {
            return ToolResult::rejected("create_note", "системные действия отключены");
        }
        let Some(name) = call.arg_str("name") else {
            return ToolResult::rejected("create_note", "не передано `name`");
        };
        if looks_destructive(&name) {
            return ToolResult::rejected("create_note", "имя содержит деструктивный паттерн");
        }
        // Чистим имя: только базовое имя без сепараторов пути.
        let safe_name = name.replace(['/', '\\', '.'], "_");
        let dir = documents_dir(&self.config);
        if let Err(e) = tokio::fs::create_dir_all(&dir).await {
            return ToolResult::rejected("create_note", format!("каталог недоступен: {e}"));
        }
        let path = dir.join(format!("{safe_name}.txt"));
        if !path_within_root(&dir, &path) {
            return ToolResult::rejected("create_note", "путь вне разрешённого каталога");
        }
        let content = call.arg_str("content").unwrap_or_default();
        match tokio::fs::write(&path, content).await {
            Ok(_) => ToolResult::ok("create_note", format!("Создала заметку: {}", path.display())),
            Err(e) => ToolResult::rejected("create_note", format!("не удалось записать: {e}")),
        }
    }
}
