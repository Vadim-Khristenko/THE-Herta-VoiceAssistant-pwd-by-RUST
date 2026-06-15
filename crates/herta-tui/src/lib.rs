//! `herta-tui` — современный терминальный интерфейс на `ratatui`/`crossterm`.
//!
//! Архитектура: чистый рендер ([`ui`]) поверх изменчивого состояния ([`state`]),
//! управляемого async-циклом ([`app::App`]) через единый `tokio::select!`-селектор.
//! Структурная конфигурация ([`theme::Theme`]) отделена от данных, что исключает
//! конфликты заимствования с `&mut Frame`.

#![forbid(unsafe_code)]

pub mod app;
pub mod state;
pub mod theme;
pub mod ui;

pub use app::App;
pub use theme::Theme;
