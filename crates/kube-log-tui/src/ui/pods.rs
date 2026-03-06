use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem};

use crate::app::{App, Focus};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();
    let title = format!(" Pods ({}) ", app.pods.len());

    let items: Vec<ListItem> = app
        .pods
        .iter()
        .map(|pod| {
            let (status_icon, status_color) = match pod.status.as_str() {
                "Running" => ("●", theme.status_running),
                "Pending" => ("◌", theme.status_pending),
                "Succeeded" => ("✓", theme.status_succeeded),
                "Failed" => ("✗", theme.status_failed),
                _ => ("?", theme.status_unknown),
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!("{status_icon} "), Style::default().fg(status_color)),
                Span::styled(pod.name.as_str(), Style::default().fg(theme.fg)),
                Span::styled(
                    format!("  {} R:{}", pod.ready, pod.restarts),
                    Style::default().fg(theme.muted),
                ),
            ]))
        })
        .collect();

    let border_color = match app.focus {
        Focus::Pods => theme.border_focused,
        _ => theme.border_unfocused,
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
                .bg(theme.highlight_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, area, &mut app.pod_list_state);
}
