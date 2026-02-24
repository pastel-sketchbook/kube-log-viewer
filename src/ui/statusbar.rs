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
        InputMode::Normal => {
            let active = Style::default().fg(theme.accent);
            let follow_label = if app.follow_mode { active } else { label };
            let wide_label = if app.wide_logs { active } else { label };

            Line::from(vec![
                Span::styled(" j/k", key),
                Span::styled(" nav  ", label),
                Span::styled("/", key),
                Span::styled(" search  ", label),
                Span::styled("n", key),
                Span::styled(" ns  ", label),
                Span::styled("c", key),
                Span::styled(" ctx  ", label),
                Span::styled("s", key),
                Span::styled(" container  ", label),
                Span::styled("f", key),
                Span::styled(" follow  ", follow_label),
                Span::styled("w", key),
                Span::styled(" wide  ", wide_label),
                Span::styled("?", key),
                Span::styled(" help  ", label),
                Span::styled("q", key),
                Span::styled(" quit", label),
            ])
        }
    };

    let paragraph = Paragraph::new(status).style(Style::default().bg(theme.statusbar_bg));

    frame.render_widget(paragraph, area);
}
