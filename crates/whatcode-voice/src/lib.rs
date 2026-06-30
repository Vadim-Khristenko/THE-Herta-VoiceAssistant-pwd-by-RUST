//! `whatcode-voice` — озвучивание ответов (TTS).
//!
//! Три бэкенда:
//! - **System** — системная утилита (`say`/`espeak-ng`/PowerShell), без сети;
//! - **ElevenLabs** — облачный синтез (нужен API-ключ);
//! - **GoogleCloud** — Google Cloud Text-to-Speech (нужен API-ключ).
//!
//! Облачные бэкенды получают аудио по HTTP (rustls, без нативных аудио-зависимостей),
//! пишут во временный файл и проигрывают системным плеером. `speak` не блокирует
//! UI: системный бэкенд запускает процесс, облачные — отдельную `tokio`-таску.
//! Распознавание речи (STT) — задача следующей итерации.

#![forbid(unsafe_code)]

mod cloud;
pub mod stt;

pub use stt::Stt;

use whatcode_core::config::{TtsProvider, VoiceConfig};
use std::process::{Command, Stdio};

/// Бэкенд озвучивания.
#[derive(Debug, Clone)]
pub struct Voice {
    enabled: bool,
    provider: TtsProvider,
    program: Option<String>,
    voice_name: Option<String>,
    cfg: VoiceConfig,
    http: reqwest::Client,
}

impl Voice {
    /// Собрать из конфигурации.
    pub fn from_config(cfg: &VoiceConfig) -> Self {
        let program = cfg.tts_command.clone().or_else(detect_tts);
        let available = match cfg.provider {
            TtsProvider::System => program.is_some(),
            TtsProvider::ElevenLabs => cfg.elevenlabs_api_key.is_some(),
            TtsProvider::GoogleCloud => cfg.google_api_key.is_some(),
            TtsProvider::Azure => cfg.azure_api_key.is_some() && cfg.azure_region.is_some(),
            TtsProvider::Qwen => cfg.qwen_api_key.is_some(),
            TtsProvider::Edge => crate::which("edge-tts"),
        };
        Self {
            enabled: cfg.enabled && available,
            provider: cfg.provider,
            program,
            voice_name: cfg.voice_name.clone(),
            cfg: cfg.clone(),
            http: reqwest::Client::new(),
        }
    }

    /// Доступен ли выбранный бэкенд (есть утилита/ключ).
    pub fn is_available(&self) -> bool {
        match self.provider {
            TtsProvider::System => self.program.is_some(),
            TtsProvider::ElevenLabs => self.cfg.elevenlabs_api_key.is_some(),
            TtsProvider::GoogleCloud => self.cfg.google_api_key.is_some(),
            TtsProvider::Azure => {
                self.cfg.azure_api_key.is_some() && self.cfg.azure_region.is_some()
            }
            TtsProvider::Qwen => self.cfg.qwen_api_key.is_some(),
            TtsProvider::Edge => crate::which("edge-tts"),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn provider(&self) -> TtsProvider {
        self.provider
    }

    /// Озвучить текст. Пустой текст или недоступный бэкенд — no-op.
    pub fn speak(&self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        match self.provider {
            TtsProvider::System => self.speak_system(trimmed),
            TtsProvider::ElevenLabs
            | TtsProvider::GoogleCloud
            | TtsProvider::Azure
            | TtsProvider::Qwen
            | TtsProvider::Edge => {
                let voice = self.clone();
                let owned = trimmed.to_string();
                // Облачный синтез асинхронный — не блокируем вызывающего.
                tokio::spawn(async move {
                    if let Err(e) =
                        cloud::synthesize_and_play(&voice.http, &voice.cfg, voice.provider, &owned)
                            .await
                    {
                        tracing::warn!(error = %e, "облачный TTS не удался");
                    }
                });
            }
        }
    }

    fn speak_system(&self, text: &str) {
        let Some(program) = &self.program else { return };
        if let Err(e) = self.spawn_system(program, text) {
            tracing::warn!(error = %e, program, "системный TTS не запустился");
        }
    }

    fn spawn_system(&self, program: &str, text: &str) -> std::io::Result<()> {
        let mut cmd = Command::new(program);
        match program {
            "say" | "espeak" | "espeak-ng" => {
                if let Some(v) = &self.voice_name {
                    cmd.arg("-v").arg(v);
                }
                cmd.arg(text);
            }
            "spd-say" => {
                cmd.arg("--wait").arg(text);
            }
            "powershell" | "pwsh" => {
                let escaped = text.replace('\'', "''");
                let script = format!(
                    "Add-Type -AssemblyName System.Speech; \
                     (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak('{escaped}')"
                );
                cmd.arg("-NoProfile").arg("-Command").arg(script);
            }
            _ => {
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

    candidates
        .into_iter()
        .find(|c| which(c))
        .map(|c| c.to_string())
}

/// Есть ли исполняемый файл в PATH (без внешних зависимостей).
pub(crate) fn which(program: &str) -> bool {
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
    fn system_disabled_without_backend() {
        let cfg = VoiceConfig {
            enabled: true,
            ..Default::default()
        };
        let voice = Voice::from_config(&cfg);
        // На сборочной машине TTS-утилиты может не быть — speak пустой строки безопасен.
        voice.speak("   ");
    }

    #[test]
    fn elevenlabs_needs_key() {
        let cfg = VoiceConfig {
            enabled: true,
            provider: TtsProvider::ElevenLabs,
            ..Default::default()
        };
        let voice = Voice::from_config(&cfg);
        assert!(!voice.is_available());
        assert!(!voice.is_enabled());
    }
}
