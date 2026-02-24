use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem};

use crate::app::{App, Focus};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = format!(" Pods ({}) ", app.pods.len());

    let items: Vec<ListItem> = app
        .pods
        .iter()
        .map(|pod| {
            let (status_icon, status_color) = match pod.status.as_str() {
                "Running" => ("●", Color::Green),
                "Pending" => ("◌", Color::Yellow),
                "Succeeded" => ("✓", Color::Blue),
                "Failed" => ("✗", Color::Red),
                _ => ("?", Color::DarkGray),
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!("{status_icon} "), Style::default().fg(status_color)),
                Span::styled(pod.name.as_str(), Style::default().fg(Color::White)),
                Span::styled(
                    format!("  {} R:{}", pod.ready, pod.restarts),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let border_color = match app.focus {
        Focus::Pods => Color::Cyan,
        _ => Color::DarkGray,
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, area, &mut app.pod_list_state);
}
