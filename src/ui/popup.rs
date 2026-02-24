use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};

use crate::app::{App, PopupKind};

/// Build styled list items, highlighting the currently-selected value.
fn styled_items<'a>(
    items: &'a [String],
    current: Option<&str>,
    highlight: Color,
    normal: Color,
) -> Vec<ListItem<'a>> {
    items
        .iter()
        .map(|item| {
            let style = match current {
                Some(c) if c == item.as_str() => {
                    Style::default().fg(highlight).add_modifier(Modifier::BOLD)
                }
                _ => Style::default().fg(normal),
            };
            ListItem::new(Span::styled(item.as_str(), style))
        })
        .collect()
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let Some(kind) = app.popup else { return };
    let theme = app.theme();

    let (title, items) = match kind {
        PopupKind::Namespaces => (
            " Namespaces ",
            styled_items(
                &app.namespaces,
                Some(&app.current_namespace),
                theme.namespace_fg,
                theme.popup_fg,
            ),
        ),
        PopupKind::Contexts => (
            " Contexts ",
            styled_items(
                &app.contexts,
                Some(&app.current_context),
                theme.context_fg,
                theme.popup_fg,
            ),
        ),
        PopupKind::Containers => (
            " Containers ",
            styled_items(
                &app.containers,
                app.selected_container.as_deref(),
                theme.search_fg,
                theme.popup_fg,
            ),
        ),
    };

    let area = super::centered_rect(40, 50, frame.area());
    frame.render_widget(Clear, area);

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.popup_border)),
        )
        .highlight_style(
            Style::default()
                .bg(theme.highlight_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, area, &mut app.popup_list_state);
}
