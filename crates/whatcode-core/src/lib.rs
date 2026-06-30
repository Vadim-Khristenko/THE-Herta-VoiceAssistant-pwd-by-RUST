//! `whatcode-core` — фундамент ассистента «Великая Герта».
//!
//! Содержит чистые, не-async строительные блоки: типы ошибок, диалоговые
//! сообщения, конфигурацию, персону, кратко-/долговременную память и движок
//! автосжатия контекста. Слои выше (LLM, инструменты, агенты, TUI) зависят
//! только от этого crate и не тянут друг друга без необходимости.
//!
//! Принципы: никаких `panic!` в библиотечном коде, только `Result`; данные
//! отделены от поведения; всё детерминированно и покрыто юнит-тестами.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod config;
pub mod context;
pub mod error;
pub mod long_memory;
pub mod memory;
pub mod message;
pub mod mode;
pub mod persona;
pub mod skill;
pub mod tool;
pub mod wakeword;

pub use config::{AppConfig, LlmProvider};
pub use context::{CompactionDecision, CompactionPlan, ContextManager};
pub use error::{WhatCodeError, Result};
pub use long_memory::{Fact, FactCategory, FactSource, LongMemoryStore};
pub use memory::DialogueMemory;
pub use message::{estimate_tokens, estimate_total_tokens, Message, Role};
pub use mode::{AgentMode, Permission, PermissionLedger, Policy, ToolRisk};
pub use skill::{load_dir as load_skills, Skill};
pub use tool::{ParamType, ToolCall, ToolParameter, ToolResult, ToolSpec};

/// Версия рабочего пространства (из Cargo).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
