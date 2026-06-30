//! Общая абстракция персоны для WhatCode.
//!
//! Персона — это изолированный модуль с каноническим лором, тоном, правилами,
//! few-shot примерами и визуальным цветом. `whatcode-core` предоставляет трейт
//! [`Persona`] и реестр, а конкретные персоны живут в отдельных файлах:
//! [`herta`](crate::persona::herta) и [`anis`](crate::persona::anis).

use crate::message::Message;

/// RGB-цвет персоны для TUI и других визуальных подсистем.
/// Не зависит от `ratatui`, чтобы ядро оставалось UI-agnostik.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersonaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl PersonaColor {
    /// Белый — нейтральный акцент по умолчанию.
    pub const WHITE: Self = Self::new(255, 255, 255);
    /// Фиолетовый — персона Герта.
    pub const HERTA_PURPLE: Self = Self::new(187, 154, 247);
    /// Жёлтый — персона Anis.
    pub const ANIS_YELLOW: Self = Self::new(249, 226, 122);
    /// Ледяной циан — резервный акцент WhatCode.
    pub const WHATCODE_CYAN: Self = Self::new(137, 221, 255);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Трейт персоны. Все реализации должны быть потокобезопасными (`Send + Sync`).
pub trait Persona: Send + Sync {
    /// Машинный идентификатор (маленькими буквами, без пробелов).
    fn id(&self) -> &'static str;
    /// Имя для отображения в TUI и системном промпте.
    fn display_name(&self) -> &'static str;
    /// Источник/франшиза персоны (например, "Honkai: Star Rail").
    fn source(&self) -> &'static str;
    /// Акцентный цвет для TUI.
    fn color(&self) -> PersonaColor;
    /// Системный промпт для LLM (полный или компактный).
    fn system_prompt(&self, model_name: Option<&str>) -> String;
    /// Стартовые сообщения: системный промпт + few-shot примеры.
    fn bootstrap_messages(
        &self,
        model_name: Option<&str>,
        long_memory_block: Option<&str>,
    ) -> Vec<Message>;
    /// Промпт «починки» персоны (если черновой ответ вышел из образа).
    fn repair_messages(&self, user_text: &str, draft_reply: &str) -> Vec<Message>;
    /// Промпт «полировки» (усилить характер без новых фактов).
    fn polish_messages(&self, user_text: &str, draft_reply: &str) -> Vec<Message>;
    /// Запрос об идентичности?
    fn is_identity_query(&self, text: &str) -> bool;
    /// Разговорный/личный вопрос?
    fn is_casual_query(&self, text: &str) -> bool;
    /// Ответ нарушил персону?
    fn needs_persona_repair(&self, reply: &str) -> bool;
    /// Жёстко заданный ответ об идентичности (если применим).
    fn build_identity_reply(&self, user_text: &str) -> Option<String>;
    /// Подсказка для живого разговорного тона.
    fn build_conversational_hint(&self, user_text: &str) -> Option<String>;
}

/// Нормализация текста для паттерн-матчинга.
pub(crate) fn normalize(text: &str) -> String {
    text.trim().to_lowercase()
}

/// Превращает список строк в маркированный блок.
pub(crate) fn bullet_block(items: &[&str]) -> String {
    items
        .iter()
        .map(|i| format!("- {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Префиксы моделей, для которых используется компактный bootstrap.
pub const COMPACT_BOOTSTRAP_MODEL_PREFIXES: &[&str] = &[
    "qwen3",
    "gemma",
    "gemini-3.1-flash-live",
    "gemini-2.5-flash-native-audio",
];

/// Использовать ли компактный bootstrap для данной модели.
pub fn should_use_compact_bootstrap(model_name: Option<&str>) -> bool {
    match model_name {
        None => false,
        Some(name) => {
            let n = normalize(name);
            COMPACT_BOOTSTRAP_MODEL_PREFIXES
                .iter()
                .any(|p| n.starts_with(p))
        }
    }
}

/// Доступные персоны по умолчанию.
fn all_personas() -> Vec<Box<dyn Persona>> {
    vec![
        Box::new(super::herta::Herta),
        Box::new(super::anis::Anis),
    ]
}

/// Получить персону по id. Если не найдена — вернёт нейтральную персону по умолчанию.
pub fn get(id: &str) -> Box<dyn Persona> {
    let id_norm = id.trim().to_lowercase();
    for p in all_personas() {
        if p.id() == id_norm {
            return p;
        }
    }
    Box::new(DefaultPersona)
}

/// Список id и имён всех доступных персон.
pub fn list() -> Vec<(&'static str, &'static str)> {
    all_personas()
        .into_iter()
        .map(|p| (p.id(), p.display_name()))
        .collect()
}

/// Нейтральная персона по умолчанию. Не выдаёт себя за конкретного персонажа,
/// но сохраняет деловой, краткий тон WhatCode.
#[derive(Debug, Clone, Copy)]
pub struct DefaultPersona;

impl Persona for DefaultPersona {
    fn id(&self) -> &'static str {
        "default"
    }

    fn display_name(&self) -> &'static str {
        "WhatCode"
    }

    fn source(&self) -> &'static str {
        "WhatCode"
    }

    fn color(&self) -> PersonaColor {
        PersonaColor::WHITE
    }

    fn system_prompt(&self, _model_name: Option<&str>) -> String {
        "Вы — WhatCode, ассистент для разработки. \n\
         Отвечайте по существу, кратко и точно. \n\
         Цените чистоту кода, модульность, типизацию и надёжность. \n\
         Никогда не выводите теги thinking или внутренние рассуждения.".to_string()
    }

    fn bootstrap_messages(
        &self,
        model_name: Option<&str>,
        long_memory_block: Option<&str>,
    ) -> Vec<Message> {
        let mut system_prompt = self.system_prompt(model_name);
        if let Some(block) = long_memory_block {
            if !block.trim().is_empty() {
                system_prompt.push_str("\n\n");
                system_prompt.push_str(block);
            }
        }
        vec![Message::system(system_prompt)]
    }

    fn repair_messages(&self, user_text: &str, draft_reply: &str) -> Vec<Message> {
        vec![
            Message::system(self.system_prompt(None)),
            Message::user(format!(
                "Запрос пользователя: {user_text}\n\n\
                 Черновой ответ: {draft_reply}\n\n\
                 Перепиши черновой ответ, сохранив смысл. Убери дружелюбный канцелярский тон. \
                 Отвечай как сдержанный, точный и полезный ассистент для разработки. \
                 Не выводи теги thinking. Верни только итоговый ответ."
            )),
        ]
    }

    fn polish_messages(&self, user_text: &str, draft_reply: &str) -> Vec<Message> {
        self.repair_messages(user_text, draft_reply)
    }

    fn is_identity_query(&self, text: &str) -> bool {
        let n = normalize(text);
        ["кто ты", "кто вы", "опиши себя", "представься", "что ты такое", "что вы такое"]
            .iter()
            .any(|p| n.contains(p))
    }

    fn is_casual_query(&self, text: &str) -> bool {
        let n = normalize(text);
        ["как дела", "привет", "здравствуй", "поговорим", "поболтаем"]
            .iter()
            .any(|p| n.contains(p))
    }

    fn needs_persona_repair(&self, reply: &str) -> bool {
        let n = normalize(reply);
        ["я ваш ассистент", "я ассистент", "языковая модель", "обычный ассистент"]
            .iter()
            .any(|p| n.contains(p))
    }

    fn build_identity_reply(&self, user_text: &str) -> Option<String> {
        let n = normalize(user_text);
        if ["бот", "программа", "ии", "нейросеть"]
            .iter()
            .any(|t| n.contains(t))
        {
            Some(
                "Вы разговариваете с WhatCode через эту оболочку. \
                 Техническая реализация вторична; важнее результат."
                    .to_string(),
            )
        } else {
            Some("WhatCode — ассистент для разработки. Расскажите, с чем помочь.".to_string())
        }
    }

    fn build_conversational_hint(&self, user_text: &str) -> Option<String> {
        if !self.is_casual_query(user_text) {
            return None;
        }
        Some(
            "Это разговорный вопрос. Ответь кратко, естественно и по-человечески. \
             Не уходи в канцелярит."
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_persona_is_neutral() {
        let p = DefaultPersona;
        assert_eq!(p.id(), "default");
        assert_eq!(p.display_name(), "WhatCode");
        assert_eq!(p.color(), PersonaColor::WHITE);
    }

    #[test]
    fn registry_returns_known_personas() {
        assert_eq!(get("herta").id(), "herta");
        assert_eq!(get("anis").id(), "anis");
        assert_eq!(get("unknown").id(), "default");
    }

    #[test]
    fn should_use_compact_bootstrap_for_qwen3() {
        assert!(should_use_compact_bootstrap(Some("qwen3:4b")));
        assert!(should_use_compact_bootstrap(Some("gemma-3-27b-it")));
        assert!(!should_use_compact_bootstrap(Some("gpt-oss-120b")));
        assert!(!should_use_compact_bootstrap(None));
    }
}
