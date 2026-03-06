use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use crate::app::{App, EXPORT_FORMAT_OPTIONS, ExportFormat, PopupKind, TIME_RANGE_OPTIONS};

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

/// Build list items for time range options, highlighting the active range.
fn time_range_items(app: &App, highlight: Color, normal: Color) -> Vec<ListItem<'static>> {
    let current_label = app.time_range.label();
    TIME_RANGE_OPTIONS
        .iter()
        .map(|&(label, _)| {
            let style = if label == current_label {
                Style::default().fg(highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(normal)
            };
            ListItem::new(Span::styled(label, style))
        })
        .collect()
}

/// Build list items for export format options (no "current" highlighting).
fn export_format_items(normal: Color) -> Vec<ListItem<'static>> {
    EXPORT_FORMAT_OPTIONS
        .iter()
        .map(|&(label, _)| ListItem::new(Span::styled(label, Style::default().fg(normal))))
        .collect()
}

/// Generate a preview filename for the given export format.
fn export_preview_filename(format: ExportFormat) -> String {
    let ext = format.extension();
    let ts = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S");
    format!("kube-log-viewer-export-{ts}.{ext}")
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let Some(kind) = app.popup else { return };

    // Copy theme colors upfront so we don't hold an immutable borrow on `app`
    // when we later need `&mut app.popup_list_state`.
    let theme = app.theme();
    let popup_border = theme.popup_border;
    let popup_fg = theme.popup_fg;
    let popup_bg = theme.bg;
    let highlight_bg = theme.highlight_bg;
    let namespace_fg = theme.namespace_fg;
    let context_fg = theme.context_fg;
    let search_fg = theme.search_fg;
    let accent = theme.accent;
    let muted = theme.muted;

    let (title, items) = match kind {
        PopupKind::Namespaces => (
            " Namespaces ",
            styled_items(
                &app.namespaces,
                Some(&app.current_namespace),
                namespace_fg,
                popup_fg,
            ),
        ),
        PopupKind::Contexts => (
            " Contexts ",
            styled_items(
                &app.contexts,
                Some(&app.current_context),
                context_fg,
                popup_fg,
            ),
        ),
        PopupKind::Containers => (
            " Containers ",
            styled_items(
                &app.containers,
                app.selected_container.as_deref(),
                search_fg,
                popup_fg,
            ),
        ),
        PopupKind::TimeRange => (" Time Range ", time_range_items(app, accent, popup_fg)),
        PopupKind::ExportFormat => (" Export Format ", export_format_items(popup_fg)),
    };

    let area = super::centered_rect(40, 50, frame.area());
    frame.render_widget(Clear, area);

    // Export format popup gets a filename preview footer
    if kind == PopupKind::ExportFormat {
        let selected_format = app
            .popup_list_state
            .selected()
            .and_then(|i| EXPORT_FORMAT_OPTIONS.get(i))
            .map(|&(_, fmt)| fmt)
            .unwrap_or(ExportFormat::PlainText);

        let preview = export_preview_filename(selected_format);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(4)])
            .split(area);

        // List portion
        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                    .border_style(Style::default().fg(popup_border))
                    .style(Style::default().bg(popup_bg)),
            )
            .highlight_style(
                Style::default()
                    .bg(highlight_bg)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");
        frame.render_stateful_widget(list, chunks[0], &mut app.popup_list_state);

        // Filename preview footer
        let preview_block = Block::default()
            .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(popup_border))
            .style(Style::default().bg(popup_bg));
        let preview_paragraph = Paragraph::new(vec![
            Line::from(vec![
                Span::styled(" File: ", Style::default().fg(muted)),
                Span::styled(preview, Style::default().fg(accent)),
            ]),
            Line::from(Span::styled(
                " Saved to the folder you ran this app from",
                Style::default().fg(muted),
            )),
        ])
        .block(preview_block);
        frame.render_widget(preview_paragraph, chunks[1]);

        return;
    }

    // Standard popup for everything else
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(popup_border))
                .style(Style::default().bg(popup_bg)),
        )
        .highlight_style(
            Style::default()
                .bg(highlight_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, area, &mut app.popup_list_state);
}
