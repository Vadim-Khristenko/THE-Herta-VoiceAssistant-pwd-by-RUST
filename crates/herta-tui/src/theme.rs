//! Тема оформления. Все цвета — явные `Color::Rgb`, никаких магических ANSI-строк.
//! Палитра «ледяной эрудиции»: глубокий космос, морозный циан, аметист Эона.

use ratatui::style::{Color, Modifier, Style};

/// Неизменяемая палитра. Передаётся в виджеты по ссылке, не клонируется на кадр.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub surface: Color,
    pub border: Color,
    pub border_focused: Color,
    pub text: Color,
    pub text_dim: Color,
    pub herta: Color,
    pub user: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: Color::Rgb(13, 17, 28),
            surface: Color::Rgb(20, 26, 40),
            border: Color::Rgb(48, 58, 84),
            border_focused: Color::Rgb(122, 162, 247),
            text: Color::Rgb(205, 214, 244),
            text_dim: Color::Rgb(110, 122, 158),
            herta: Color::Rgb(137, 221, 255),
            user: Color::Rgb(158, 206, 106),
            accent: Color::Rgb(187, 154, 247),
            success: Color::Rgb(158, 206, 106),
            warning: Color::Rgb(224, 175, 104),
            error: Color::Rgb(247, 118, 142),
        }
    }
}

impl Theme {
    pub fn base(&self) -> Style {
        Style::default().fg(self.text).bg(self.bg)
    }

    pub fn border(&self, focused: bool) -> Style {
        Style::default().fg(if focused { self.border_focused } else { self.border })
    }

    pub fn title(&self) -> Style {
        Style::default().fg(self.accent).add_modifier(Modifier::BOLD)
    }

    pub fn dim(&self) -> Style {
        Style::default().fg(self.text_dim)
    }

    pub fn herta_label(&self) -> Style {
        Style::default().fg(self.herta).add_modifier(Modifier::BOLD)
    }

    pub fn user_label(&self) -> Style {
        Style::default().fg(self.user).add_modifier(Modifier::BOLD)
    }
}
