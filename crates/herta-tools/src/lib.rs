//! `herta-tools` — фреймворк вызова инструментов и встроенные инструменты.
//!
//! Каждый инструмент реализует async-трейт [`Tool`]. [`ToolRegistry`] хранит их,
//! отдаёт схемы модели и диспетчеризует вызовы, отклоняя деструктивные действия.
//! [`build_registry`] собирает полный набор по конфигурации.

#![forbid(unsafe_code)]

pub mod builder;
pub mod code_tools;
pub mod fs_tools;
pub mod git;
pub mod http_tool;
pub mod memory_tools;
pub mod registry;
pub mod safety;
pub mod skills;
pub mod system_actions;
pub mod time_tool;
pub mod util;
pub mod web_search;

pub use builder::build_registry;
pub use registry::{Tool, ToolRegistry};
pub use skills::SkillLibrary;
