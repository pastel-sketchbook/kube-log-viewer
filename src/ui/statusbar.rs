use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::app::{App, InputMode};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    let key = Style::default().fg(theme.statusbar_key);
    let label = Style::default().fg(theme.statusbar_label);

    let status = match app.input_mode {
        InputMode::Search => Line::from(vec![
            Span::styled(" /", Style::default().fg(theme.search_fg)),
            Span::styled(
                app.search_query.as_str(),
                Style::default().fg(theme.search_fg),
            ),
            Span::styled(" | ", label),
            Span::styled("Enter", key),
            Span::styled(" confirm  ", label),
            Span::styled("Esc", key),
            Span::styled(" cancel", label),
            Span::styled(format!(" [{}]", theme.name), label),
        ]),
        InputMode::Normal => Line::from(vec![
            Span::styled(" ↑↓", key),
            Span::styled(" nav  ", label),
            Span::styled("Enter", key),
            Span::styled(" select  ", label),
            Span::styled("Tab", key),
            Span::styled(" focus  ", label),
            Span::styled("/", key),
            Span::styled(" search  ", label),
            Span::styled("n", key),
            Span::styled(" ns  ", label),
            Span::styled("c", key),
            Span::styled(" ctx  ", label),
            Span::styled("s", key),
            Span::styled(" container  ", label),
            Span::styled("f", key),
            Span::styled(" follow  ", label),
            Span::styled("w", key),
            Span::styled(" wide  ", label),
            Span::styled("t", key),
            Span::styled(" theme  ", label),
            Span::styled("?", key),
            Span::styled(" help  ", label),
            Span::styled("q", key),
            Span::styled(" quit", label),
            Span::styled(format!(" [{}]", theme.name), label),
        ]),
    };

    let paragraph = Paragraph::new(status).style(Style::default().bg(theme.statusbar_bg));

    frame.render_widget(paragraph, area);
}
