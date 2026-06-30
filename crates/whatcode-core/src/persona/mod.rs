//! Абстракция персон WhatCode.
//!
//! Персона — это изолированный модуль с каноническим лором, тоном, правилами,
//! few-shot примерами и визуальным цветом. Конкретные реализации живут в
//! [`herta`] и [`anis`]; общий трейт и реестр — в [`common`].
//!
//! Для обратной совместимости модуль также предоставляет функции по умолчанию,
//! делегирующие к персоне [`Herta`].

pub mod anis;
pub mod common;
pub mod herta;

pub use common::{Persona, PersonaColor};
pub use herta::Herta;
pub use anis::Anis;

use crate::message::Message;

/// Персона по умолчанию для обратной совместимости — Герта.
fn default_persona() -> Box<dyn Persona> {
    common::get("herta")
}

// --- обратно-совместимые делегаты (старые функции модуля `persona`) ---

pub fn is_identity_query(text: &str) -> bool {
    default_persona().is_identity_query(text)
}

pub fn is_casual_query(text: &str) -> bool {
    default_persona().is_casual_query(text)
}

pub fn needs_persona_repair(reply: &str) -> bool {
    default_persona().needs_persona_repair(reply)
}

pub fn build_conversational_hint(user_text: &str) -> Option<String> {
    default_persona().build_conversational_hint(user_text)
}

pub fn build_identity_reply(user_text: &str) -> String {
    default_persona()
        .build_identity_reply(user_text)
        .unwrap_or_else(|| "WhatCode — ассистент для разработки.".to_string())
}

pub fn should_use_compact_bootstrap(model_name: Option<&str>) -> bool {
    common::should_use_compact_bootstrap(model_name)
}

pub fn build_system_prompt(model_name: Option<&str>) -> String {
    default_persona().system_prompt(model_name)
}

pub fn build_bootstrap_messages(
    model_name: Option<&str>,
    long_memory_block: Option<&str>,
) -> Vec<Message> {
    default_persona().bootstrap_messages(model_name, long_memory_block)
}

pub fn build_persona_repair_messages(user_text: &str, draft_reply: &str) -> Vec<Message> {
    default_persona().repair_messages(user_text, draft_reply)
}

pub fn build_persona_polish_messages(user_text: &str, draft_reply: &str) -> Vec<Message> {
    default_persona().polish_messages(user_text, draft_reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backward_compat_identity_query() {
        assert!(is_identity_query("Кто ты?"));
    }

    #[test]
    fn backward_compat_bootstrap_is_herta() {
        let msgs = build_bootstrap_messages(Some("gpt-oss-120b"), None);
        assert_eq!(msgs[0].role, crate::message::Role::System);
        assert!(msgs[0].content.contains("Великая Герта"));
    }

    #[test]
    fn can_load_anis() {
        let p = common::get("anis");
        assert_eq!(p.id(), "anis");
        assert_eq!(p.color(), PersonaColor::ANIS_YELLOW);
    }

    #[test]
    fn default_persona_is_herta() {
        let p = common::get("default");
        // default возвращает DefaultPersona, а get("herta") — Herta
        assert_eq!(p.id(), "default");
    }
}
