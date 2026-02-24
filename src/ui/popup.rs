use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};

use crate::app::{App, PopupKind};

pub fn render(frame: &mut Frame, app: &mut App) {
    let Some(kind) = app.popup else { return };

    let (title, items) = match kind {
        PopupKind::Namespaces => {
            let items: Vec<ListItem> = app
                .namespaces
                .iter()
                .map(|ns| {
                    let style = if ns == &app.current_namespace {
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Span::styled(ns.as_str(), style))
                })
                .collect();
            (" Namespaces ", items)
        }
        PopupKind::Contexts => {
            let items: Vec<ListItem> = app
                .contexts
                .iter()
                .map(|ctx| {
                    let style = if ctx == &app.current_context {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Span::styled(ctx.as_str(), style))
                })
                .collect();
            (" Contexts ", items)
        }
        PopupKind::Containers => {
            let items: Vec<ListItem> = app
                .containers
                .iter()
                .map(|c| {
                    let style = if app.selected_container.as_deref() == Some(c.as_str()) {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Span::styled(c.as_str(), style))
                })
                .collect();
            (" Containers ", items)
        }
    };

    let area = super::centered_rect(40, 50, frame.area());
    frame.render_widget(Clear, area);

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, area, &mut app.popup_list_state);
}
