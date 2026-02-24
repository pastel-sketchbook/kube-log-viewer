use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let context = match app.current_context.as_str() {
        "" => "loading...",
        ctx => ctx,
    };

    let namespace = &app.current_namespace;

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" ctx: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            context,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
        Span::styled("ns: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            namespace.as_str(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
        Span::styled("? help", Style::default().fg(Color::DarkGray)),
    ]))
    .block(
        Block::default()
            .title(" kube-log-viewer ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue)),
    );

    frame.render_widget(header, area);
}
