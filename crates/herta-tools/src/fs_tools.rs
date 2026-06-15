//! Файловые инструменты только для чтения: `read_file` и `list_dir`.
//! Доступ ограничен текущим рабочим каталогом (защита от path traversal).

use crate::registry::Tool;
use crate::safety::path_within_root;
use crate::util::{truncate, MAX_OUTPUT_CHARS};
use async_trait::async_trait;
use herta_core::{ParamType, ToolCall, ToolParameter, ToolResult, ToolSpec};
use std::path::PathBuf;

fn root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn resolve(rel: &str) -> Option<PathBuf> {
    if rel.contains("..") {
        return None;
    }
    let base = root();
    let candidate = base.join(rel);
    if path_within_root(&base, &candidate) {
        Some(candidate)
    } else {
        None
    }
}

/// `read_file` — прочитать текстовый файл в пределах проекта.
#[derive(Default)]
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "read_file",
            "Прочитать содержимое текстового файла относительно корня проекта (текущего рабочего \
             каталога). Только чтение. Путь не должен выходить за пределы проекта и содержать `..`. \
             Вывод усекается, если файл слишком большой. Используй, чтобы посмотреть код или конфиг \
             перед анализом.",
            vec![ToolParameter::new("path", ParamType::String, "Путь к файлу относительно корня проекта", true)],
        )
    }

    async fn call(&self, call: &ToolCall) -> ToolResult {
        let Some(rel) = call.arg_str("path") else {
            return ToolResult::rejected("read_file", "не передан `path`");
        };
        let Some(path) = resolve(&rel) else {
            return ToolResult::rejected("read_file", "путь вне корня проекта или содержит `..`");
        };
        match tokio::fs::read(&path).await {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                ToolResult::ok("read_file", truncate(text))
            }
            Err(e) => ToolResult::rejected("read_file", format!("не удалось прочитать: {e}")),
        }
    }
}

/// `list_dir` — перечислить содержимое каталога в пределах проекта.
#[derive(Default)]
pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "list_dir",
            "Перечислить файлы и подкаталоги в каталоге относительно корня проекта. Только чтение. \
             Каталоги помечены завершающим `/`. По умолчанию листится корень проекта. Используй для \
             разведки структуры репозитория.",
            vec![ToolParameter::new("path", ParamType::String, "Каталог относительно корня (по умолчанию корень)", false)],
        )
    }

    async fn call(&self, call: &ToolCall) -> ToolResult {
        let rel = call.arg_str("path").unwrap_or_else(|| ".".to_string());
        let Some(dir) = resolve(&rel) else {
            return ToolResult::rejected("list_dir", "путь вне корня проекта или содержит `..`");
        };
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(rd) => rd,
            Err(e) => {
                return ToolResult::rejected("list_dir", format!("не удалось открыть каталог: {e}"))
            }
        };
        let mut names: Vec<String> = Vec::new();
        loop {
            match entries.next_entry().await {
                Ok(Some(entry)) => {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                    names.push(if is_dir { format!("{name}/") } else { name });
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
        names.sort();
        let listing = if names.is_empty() {
            "(пусто)".to_string()
        } else {
            names.join("\n")
        };
        // Усечение на случай гигантских каталогов.
        let listing = if listing.chars().count() > MAX_OUTPUT_CHARS {
            truncate(listing)
        } else {
            listing
        };
        ToolResult::ok("list_dir", listing)
    }
}
