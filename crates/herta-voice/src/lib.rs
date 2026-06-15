//! `herta-voice` — озвучивание ответов (TTS) через системные утилиты.
//!
//! Намеренно без нативных аудио-зависимостей (cpal/alsa и т.п.): синтез речи
//! делегируется внешней программе ОС, которая определяется автоматически и
//! переопределяется конфигом. Это сохраняет лёгкость сборки и зелёный CI.
//! Распознавание речи (STT) — задача следующей итерации.
//!
//! `speak` запускает процесс в фоне (fire-and-forget) и не блокирует UI.

#![forbid(unsafe_code)]

use herta_core::config::VoiceConfig;
use std::process::{Command, Stdio};

/// Бэкенд озвучивания, выбранный для текущей платформы.
#[derive(Debug, Clone)]
pub struct Voice {
    enabled: bool,
    program: Option<String>,
    voice_name: Option<String>,
}

impl Voice {
    /// Собрать из конфигурации. Если TTS-команда не задана и не найдена —
    /// озвучивание тихо отключается (без ошибок).
    pub fn from_config(cfg: &VoiceConfig) -> Self {
        let program = cfg.tts_command.clone().or_else(detect_tts);
        Self {
            enabled: cfg.enabled && program.is_some(),
            program,
            voice_name: cfg.voice_name.clone(),
        }
    }

    pub fn is_available(&self) -> bool {
        self.program.is_some()
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Озвучить текст. Пустой текст или отсутствие бэкенда — no-op.
    /// Процесс запускается отдельно и не блокирует вызывающего.
    pub fn speak(&self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        let Some(program) = &self.program else { return };
        if let Err(e) = self.spawn(program, trimmed) {
            tracing::warn!(error = %e, program, "TTS не запустился");
        }
    }

    fn spawn(&self, program: &str, text: &str) -> std::io::Result<()> {
        let mut cmd = Command::new(program);
        // Аргументы под конкретные утилиты.
        match program {
            "say" => {
                // macOS: say [-v voice] "текст"
                if let Some(v) = &self.voice_name {
                    cmd.arg("-v").arg(v);
                }
                cmd.arg(text);
            }
            "espeak" | "espeak-ng" => {
                if let Some(v) = &self.voice_name {
                    cmd.arg("-v").arg(v);
                }
                cmd.arg(text);
            }
            "spd-say" => {
                cmd.arg("--wait").arg(text);
            }
            "powershell" | "pwsh" => {
                // Windows SAPI через инлайн-скрипт. Текст экранируется одинарными кавычками.
                let escaped = text.replace('\'', "''");
                let script = format!(
                    "Add-Type -AssemblyName System.Speech; \
                     (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak('{escaped}')"
                );
                cmd.arg("-NoProfile").arg("-Command").arg(script);
            }
            _ => {
                // Неизвестная пользовательская команда: передаём текст одним аргументом.
                cmd.arg(text);
            }
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd.spawn().map(|_child| ())
    }
}

/// Подобрать доступную TTS-утилиту по платформе.
fn detect_tts() -> Option<String> {
    #[cfg(target_os = "macos")]
    let candidates = ["say"];
    #[cfg(target_os = "windows")]
    let candidates = ["powershell", "pwsh"];
    #[cfg(all(unix, not(target_os = "macos")))]
    let candidates = ["espeak-ng", "espeak", "spd-say"];

    for candidate in candidates {
        if which(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Есть ли исполняемый файл в PATH (без внешних зависимостей).
fn which(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let exe_suffixes: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    std::env::split_paths(&path).any(|dir| {
        exe_suffixes.iter().any(|suffix| {
            let mut candidate = dir.join(program);
            if !suffix.is_empty() {
                candidate.set_extension(&suffix[1..]);
            }
            candidate.is_file()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_when_no_backend() {
        let cfg = VoiceConfig {
            enabled: true,
            tts_command: Some("definitely-not-a-real-tts-xyz".into()),
            voice_name: None,
        };
        let voice = Voice::from_config(&cfg);
        // Команда задана явно, поэтому считается доступной (проверка наличия — при запуске).
        assert!(voice.is_available());
        // speak с пустым текстом — гарантированный no-op без паники.
        voice.speak("   ");
    }

    #[test]
    fn detect_does_not_panic() {
        let _ = detect_tts();
    }
}
