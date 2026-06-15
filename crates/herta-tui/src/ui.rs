//! Чистая функция рендера: `AppState` + `Theme` -> кадр. Никаких мутаций
//! состояния здесь. Раскладка строится жёсткими `Constraint::Length`/`Min`,
//! чтобы исключить переполнение и панику по границам сетки.

use crate::state::{AppState, Focus, LineKind};
use crate::theme::Theme;
use herta_agent::AgentStatus;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap};
use ratatui::Frame;

/// Оценка числа экранных строк для текста при заданной ширине (для прокрутки).
/// Ширина 0 трактуется как 1, чтобы исключить деление на ноль.
fn wrapped_rows(text: &str, width: u16) -> u16 {
    let w = width.max(1) as usize;
    let mut rows: usize = 0;
    for segment in text.split('\n') {
        let chars = segment.chars().count();
        rows += chars.div_ceil(w).max(1);
    }
    rows.min(u16::MAX as usize) as u16
}

/// Главная точка рендера.
pub fn render(frame: &mut Frame, state: &AppState, theme: &Theme) {
    let area = frame.area();
    // Фон.
    frame.render_widget(Block::default().style(theme.base()), area);

    // Вертикальная раскладка: шапка, тело, ввод, статус.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // body
            Constraint::Length(3), // input
            Constraint::Length(1), // status bar
        ])
        .split(area);

    render_header(frame, rows[0], state, theme);
    render_body(frame, rows[1], state, theme);
    render_input(frame, rows[2], state, theme);
    render_status(frame, rows[3], state, theme);
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border(false));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let title = Line::from(vec![
        Span::styled("❄ THE HERTA ", theme.title()),
        Span::styled("· Эманатор Эрудиции · голосовой ассистент", theme.dim()),
    ]);
    let meta = Line::from(vec![
        Span::styled("провайдер ", theme.dim()),
        Span::styled(&state.provider_label, Style::default().fg(theme.accent)),
        Span::styled("  модель ", theme.dim()),
        Span::styled(&state.model_label, Style::default().fg(theme.herta)),
    ]);
    let para = Paragraph::new(Text::from(vec![title, meta]));
    frame.render_widget(para, inner);
}

fn render_body(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    // Горизонтально: лента диалога (растягивается) + панель агентов (фикс).
    let show_agents = !state.agents.is_empty();
    let cols = if show_agents {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20), Constraint::Length(34)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20)])
            .split(area)
    };

    render_transcript(frame, cols[0], state, theme);
    if show_agents {
        render_agents(frame, cols[1], state, theme);
    }
}

fn render_transcript(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let focused = state.focus == Focus::Transcript;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border(focused))
        .title(Span::styled(" Диалог ", theme.title()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    for entry in &state.lines {
        let (label, label_style, body_style) = match entry.kind {
            LineKind::User => ("Вы", theme.user_label(), Style::default().fg(theme.text)),
            LineKind::Herta => (
                "Герта",
                theme.herta_label(),
                Style::default().fg(theme.text),
            ),
            LineKind::Notice => ("·", theme.dim(), theme.dim()),
            LineKind::ErrorNote => (
                "!",
                Style::default().fg(theme.error),
                Style::default().fg(theme.error),
            ),
        };
        // Заголовок реплики.
        if matches!(entry.kind, LineKind::User | LineKind::Herta) {
            lines.push(Line::from(Span::styled(format!("{label}:"), label_style)));
        }
        for sub in entry.text.split('\n') {
            lines.push(Line::from(Span::styled(sub.to_string(), body_style)));
        }
        lines.push(Line::from(""));
    }

    // Прижатие к низу: считаем суммарную высоту с учётом переноса.
    let viewport = inner.height;
    let total: u16 = lines
        .iter()
        .map(|l| {
            wrapped_rows(
                &l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>(),
                inner.width,
            )
        })
        .sum::<u16>();
    let max_offset = total.saturating_sub(viewport);
    let offset = max_offset.saturating_sub(state.scroll_back);

    let para = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((offset, 0));
    frame.render_widget(para, inner);
}

fn render_agents(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border(false))
        .title(Span::styled(" Марионетки ", theme.title()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    for agent in &state.agents {
        let (glyph, style) = match agent.status {
            AgentStatus::Pending => ("◌", theme.dim()),
            AgentStatus::Running => ("◐", Style::default().fg(theme.warning)),
            AgentStatus::Done => ("●", Style::default().fg(theme.success)),
            AgentStatus::Error => ("✖", Style::default().fg(theme.error)),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{glyph} "), style),
            Span::styled(agent.title.clone(), Style::default().fg(theme.text)),
        ]));
        if !agent.preview.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("  {}", agent.preview),
                theme.dim(),
            )));
        }
    }
    let para = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true });
    frame.render_widget(para, inner);
}

fn render_input(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let focused = state.focus == Focus::Input;
    let hint = if state.busy {
        " Герта размышляет… "
    } else {
        " Ввод (Enter — отправить) "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border(focused))
        .title(Span::styled(hint, theme.title()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let prompt = Line::from(vec![
        Span::styled("❯ ", Style::default().fg(theme.accent)),
        Span::styled(state.input.clone(), Style::default().fg(theme.text)),
    ]);
    frame.render_widget(Paragraph::new(prompt), inner);

    // Каретка в позиции ввода (только в фокусе и не во время запроса).
    if focused && !state.busy {
        let cursor_x = inner
            .x
            .saturating_add(2)
            .saturating_add(state.input.chars().count() as u16);
        let cursor_x = cursor_x.min(inner.x + inner.width.saturating_sub(1));
        frame.set_cursor_position((cursor_x, inner.y));
    }
}

fn render_status(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    // Слева — статус, справа — индикатор контекста.
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(26)])
        .split(area);

    let status = Line::from(vec![
        Span::styled(" ", theme.dim()),
        Span::styled(&state.status, Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(Paragraph::new(status), cols[0]);

    let ratio = state.context_ratio();
    let gauge_style = if ratio > 0.85 {
        Style::default().fg(theme.error)
    } else if ratio > 0.7 {
        Style::default().fg(theme.warning)
    } else {
        Style::default().fg(theme.success)
    };
    let gauge = Gauge::default()
        .gauge_style(gauge_style)
        .ratio(ratio as f64)
        .label(format!(
            "ctx {}/{}",
            state.context_used, state.context_limit
        ));
    frame.render_widget(gauge, cols[1]);
}

/// Модальная справка поверх всего.
pub fn render_help(frame: &mut Frame, theme: &Theme) {
    let area = frame.area();
    let w = 60u16.min(area.width.saturating_sub(4));
    let h = 14u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let popup = Rect {
        x,
        y,
        width: w,
        height: h,
    };

    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border(true))
        .title(Span::styled(" Справка ", theme.title()))
        .style(Style::default().bg(theme.surface));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let lines = vec![
        Line::from(Span::styled(
            "Enter      отправить запрос",
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            "Esc / F1   закрыть справку",
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            "Tab        переключить фокус ленты/ввода",
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            "PgUp/PgDn  прокрутка диалога",
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            "Ctrl+C     выход",
            Style::default().fg(theme.text),
        )),
        Line::from(""),
        Line::from(Span::styled("Команды:", theme.title())),
        Line::from(Span::styled(
            "/goal <текст>   задать цель и план",
            theme.dim(),
        )),
        Line::from(Span::styled(
            "/ask <текст>    вопрос саб-агенту",
            theme.dim(),
        )),
        Line::from(Span::styled(
            "/agent <текст>  задача саб-агенту",
            theme.dim(),
        )),
        Line::from(Span::styled(
            "/tools          список инструментов",
            theme.dim(),
        )),
        Line::from(Span::styled("/compact        сжать контекст", theme.dim())),
        Line::from(Span::styled("/say <текст>    озвучить (TTS)", theme.dim())),
        Line::from(Span::styled(
            "/model          модель и контекст",
            theme.dim(),
        )),
        Line::from(Span::styled(
            "/clear /quit    очистить / выход",
            theme.dim(),
        )),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }),
        inner,
    );
}
