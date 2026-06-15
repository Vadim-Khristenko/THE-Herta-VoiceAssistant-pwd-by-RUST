//! Общие утилиты инструментов: запуск внешних команд с таймаутом и усечением.

use std::time::Duration;
use tokio::process::Command;

/// Потолок длины вывода инструмента (символы), чтобы не раздувать контекст модели.
pub const MAX_OUTPUT_CHARS: usize = 4000;

/// Усечь длинный вывод по символам (не байтам — безопасно для кириллицы).
pub fn truncate(mut s: String) -> String {
    if s.chars().count() > MAX_OUTPUT_CHARS {
        s = s.chars().take(MAX_OUTPUT_CHARS).collect::<String>();
        s.push_str("\n… (вывод обрезан)");
    }
    s
}

/// Результат запуска внешней команды.
pub struct CommandOutcome {
    pub success: bool,
    pub combined: String,
}

/// Запустить команду `program args...` в каталоге `cwd` с таймаутом.
/// stdout и stderr объединяются и усекаются. Никаких паник — только `Result`-подобный
/// `CommandOutcome`/`Err(String)`.
pub async fn run_capture(
    program: &str,
    args: &[&str],
    cwd: Option<&std::path::Path>,
    timeout_secs: u64,
) -> Result<CommandOutcome, String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let fut = cmd.output();
    match tokio::time::timeout(Duration::from_secs(timeout_secs.max(1)), fut).await {
        Err(_) => Err(format!("таймаут команды `{program}`")),
        Ok(Err(e)) => Err(format!("не удалось запустить `{program}`: {e}")),
        Ok(Ok(output)) => {
            let mut combined = String::new();
            combined.push_str(&String::from_utf8_lossy(&output.stdout));
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&stderr);
            }
            Ok(CommandOutcome {
                success: output.status.success(),
                combined: truncate(combined.trim().to_string()),
            })
        }
    }
}
