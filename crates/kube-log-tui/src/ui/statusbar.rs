use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::app::{App, InputMode};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    let key = Style::default().fg(theme.statusbar_key);
    let label = Style::default().fg(theme.statusbar_label);

    let mut spans: Vec<Span> = match app.input_mode {
        InputMode::Search => vec![
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
        ],
        InputMode::Normal => {
            let active = Style::default().fg(theme.accent);
            let follow_label = if app.follow_mode { active } else { label };
            let wide_label = if app.wide_logs { active } else { label };

            vec![
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
            ]
        }
    };

    // Right-align indicators + [Theme Name]
    let health_tag = if app.hide_health_checks {
        "[HEALTH HIDDEN] "
    } else {
        ""
    };
    let theme_tag = format!("[{}] ", theme.name);
    let left_width: usize = spans.iter().map(|s| s.content.len()).sum();
    let right_width = health_tag.len() + theme_tag.len();
    let padding = (area.width as usize).saturating_sub(left_width + right_width);
    if padding > 0 {
        spans.push(Span::raw(" ".repeat(padding)));
    }
    if !health_tag.is_empty() {
        spans.push(Span::styled(health_tag, Style::default().fg(theme.accent)));
    }
    spans.push(Span::styled(theme_tag, label));

    let status = Line::from(spans);
    let paragraph = Paragraph::new(status).style(Style::default().bg(theme.statusbar_bg));

    frame.render_widget(paragraph, area);
}
