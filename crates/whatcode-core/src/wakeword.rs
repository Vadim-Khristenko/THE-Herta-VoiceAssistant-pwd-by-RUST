//! Базовый wake-word detector на тексте.
//!
//! Замена Python-модулей `wakeword/matcher.py` и `wakeword/coordinator.py`.
//! Реализация простая: проверяет, содержится ли одна из фраз в нормализованном
//! тексте. Не требует внешних библиотек и работает на любой платформе.
//!
//! Будущие итерации могут добавить Porcupine (native) или WebRTC VAD + STT
//! pipeline для real-time audio.

use crate::config::WakeWordConfig;

/// Состояние wake-word detector.
#[derive(Debug, Clone)]
pub struct WakeWordDetector {
    phrases: Vec<String>,
    follow_up_seconds: f64,
    last_trigger: Option<std::time::Instant>,
}

impl WakeWordDetector {
    /// Создать детектор из конфига.
    pub fn from_config(cfg: &WakeWordConfig) -> Self {
        Self {
            phrases: cfg.phrases.clone(),
            follow_up_seconds: cfg.follow_up_seconds,
            last_trigger: None,
        }
    }

    /// Проверить текст на наличие wake-word.
    ///
    /// Возвращает `true`, если фраза найдена или если мы всё ещё внутри
    /// follow-up окна после предыдущего срабатывания.
    pub fn check(&mut self, text: &str) -> bool {
        let normalized = normalize(text);
        for phrase in &self.phrases {
            if normalized.contains(&normalize(phrase)) {
                self.last_trigger = Some(std::time::Instant::now());
                return true;
            }
        }

        // Follow-up window: после срабатывания некоторое время любая реплика
        // считается адресованной ассистенту.
        if let Some(last) = self.last_trigger {
            let elapsed = last.elapsed().as_secs_f64();
            if elapsed < self.follow_up_seconds {
                return true;
            }
        }

        false
    }

    /// Сбросить follow-up окно.
    pub fn reset(&mut self) {
        self.last_trigger = None;
    }

    /// Показать текущие активные фразы.
    pub fn phrases(&self) -> &[String] {
        &self.phrases
    }

    /// Обновить список фраз (например, при смене персоны).
    pub fn set_phrases(&mut self, phrases: Vec<String>) {
        self.phrases = phrases;
    }
}

/// Нормализация текста: lowercase, удаление знаков препинания и лишних пробелов.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_alphanumeric() || c.is_whitespace() {
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WakeWordConfig;

    fn test_config() -> WakeWordConfig {
        WakeWordConfig {
            enabled: true,
            mode: "text".into(),
            phrases: vec!["герта".into(), "herta".into()],
            follow_up_seconds: 10.0,
        }
    }

    #[test]
    fn detects_exact_phrase() {
        let mut d = WakeWordDetector::from_config(&test_config());
        assert!(d.check("Привет, Герта, как дела?"));
    }

    #[test]
    fn detects_with_punctuation() {
        let mut d = WakeWordDetector::from_config(&test_config());
        assert!(d.check("Герта!!! Сделай это."));
    }

    #[test]
    fn no_false_positive() {
        let mut d = WakeWordDetector::from_config(&test_config());
        assert!(!d.check("Просто разговор без имени."));
    }

    #[test]
    fn follow_up_window() {
        let mut d = WakeWordDetector::from_config(&test_config());
        assert!(d.check("Герта"));
        assert!(d.check("Продолжи")); // внутри follow-up окна
        d.reset();
        assert!(!d.check("Продолжи"));
    }
}
