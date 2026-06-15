//! `herta-tools` — фреймворк вызова инструментов и встроенные инструменты.
//!
//! Каждый инструмент реализует async-трейт [`Tool`]. [`ToolRegistry`] хранит их,
//! отдаёт схемы модели и диспетчеризует вызовы, отклоняя деструктивные действия.

#![forbid(unsafe_code)]

pub mod code_tools;
pub mod memory_tools;
pub mod registry;
pub mod safety;
pub mod system_actions;
pub mod web_search;

pub use registry::{Tool, ToolRegistry};
