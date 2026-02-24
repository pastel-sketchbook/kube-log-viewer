use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::app::{App, InputMode};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let status = if app.input_mode == InputMode::Search {
        Line::from(vec![
            Span::styled(" /", Style::default().fg(Color::Yellow)),
            Span::styled(
                app.search_query.as_str(),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::styled(" confirm  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::styled(" cancel", Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ↑↓", Style::default().fg(Color::Cyan)),
            Span::styled(" nav  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::styled(" select  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::styled(" focus  ", Style::default().fg(Color::DarkGray)),
            Span::styled("/", Style::default().fg(Color::Cyan)),
            Span::styled(" search  ", Style::default().fg(Color::DarkGray)),
            Span::styled("n", Style::default().fg(Color::Cyan)),
            Span::styled(" ns  ", Style::default().fg(Color::DarkGray)),
            Span::styled("c", Style::default().fg(Color::Cyan)),
            Span::styled(" ctx  ", Style::default().fg(Color::DarkGray)),
            Span::styled("s", Style::default().fg(Color::Cyan)),
            Span::styled(" container  ", Style::default().fg(Color::DarkGray)),
            Span::styled("f", Style::default().fg(Color::Cyan)),
            Span::styled(" follow  ", Style::default().fg(Color::DarkGray)),
            Span::styled("?", Style::default().fg(Color::Cyan)),
            Span::styled(" help  ", Style::default().fg(Color::DarkGray)),
            Span::styled("q", Style::default().fg(Color::Cyan)),
            Span::styled(" quit", Style::default().fg(Color::DarkGray)),
        ])
    };

    let paragraph = Paragraph::new(status).style(Style::default().bg(Color::Rgb(30, 30, 30)));

    frame.render_widget(paragraph, area);
}
