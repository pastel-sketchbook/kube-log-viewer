use chrono::Local;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let context = match app.current_context.as_str() {
        "" => "loading...",
        ctx => ctx,
    };

    let namespace = &app.current_namespace;

    let left = Line::from(vec![
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
    ]);

    let block = Block::default()
        .title(" kube-log-viewer ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Left-aligned context & namespace
    frame.render_widget(Paragraph::new(left), inner);

    // Right-aligned version & datetime
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let right = Line::from(vec![
        Span::styled(format!("v{VERSION}"), Style::default().fg(Color::DarkGray)),
        Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
        Span::styled(now, Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
    ]);

    frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), inner);
}
