use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, Focus, InputMode};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let title = match (&app.selected_pod, &app.selected_container) {
        (Some(pod), Some(container)) => format!(" Logs: {} / {} ", pod, container),
        (Some(pod), None) => format!(" Logs: {} ", pod),
        _ => " Logs ".to_string(),
    };

    let follow_indicator = if app.follow_mode { " [FOLLOW] " } else { "" };
    let search_indicator = if !app.search_query.is_empty() {
        format!(" [/{}] ", app.search_query)
    } else {
        String::new()
    };

    let is_focused = app.focus == Focus::Logs;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let filtered_lines = app.filtered_log_lines();
    let total_lines = filtered_lines.len();

    // Calculate visible window
    let inner_height = area.height.saturating_sub(2) as usize; // subtract borders
    let scroll_offset = if app.follow_mode {
        total_lines.saturating_sub(inner_height)
    } else {
        app.log_scroll_offset
            .min(total_lines.saturating_sub(inner_height))
    };

    let visible_lines: Vec<Line> = filtered_lines
        .iter()
        .skip(scroll_offset)
        .take(inner_height)
        .map(|line| {
            if !app.search_query.is_empty() {
                highlight_search(line, &app.search_query)
            } else {
                Line::from(colorize_log_line(line))
            }
        })
        .collect();

    let block = Block::default()
        .title(format!("{}{}{}", title, follow_indicator, search_indicator))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let mut paragraph = Paragraph::new(visible_lines).block(block);

    if app.wrap_lines {
        paragraph = paragraph.wrap(Wrap { trim: false });
    }

    frame.render_widget(paragraph, area);

    // Render search input bar when in search mode
    if app.input_mode == InputMode::Search {
        render_search_input(frame, app, area);
    }
}

fn render_search_input(frame: &mut Frame, app: &App, area: Rect) {
    let input_area = Rect {
        x: area.x + 1,
        y: area.y + area.height.saturating_sub(2),
        width: area.width.saturating_sub(2),
        height: 1,
    };

    let input = Paragraph::new(format!("/{}", app.search_query))
        .style(Style::default().fg(Color::Yellow).bg(Color::DarkGray));

    frame.render_widget(input, input_area);
}

fn colorize_log_line(line: &str) -> Vec<Span<'_>> {
    if line.contains("ERROR") || line.contains("error") {
        vec![Span::styled(line, Style::default().fg(Color::Red))]
    } else if line.contains("WARN") || line.contains("warn") {
        vec![Span::styled(line, Style::default().fg(Color::Yellow))]
    } else if line.contains("DEBUG") || line.contains("debug") {
        vec![Span::styled(line, Style::default().fg(Color::DarkGray))]
    } else {
        vec![Span::raw(line)]
    }
}

fn highlight_search<'a>(line: &'a str, query: &str) -> Line<'a> {
    let lower_line = line.to_lowercase();
    let lower_query = query.to_lowercase();

    let mut spans = Vec::new();
    let mut last_end = 0;

    for (start, _) in lower_line.match_indices(&lower_query) {
        if start > last_end {
            spans.push(Span::raw(&line[last_end..start]));
        }
        let end = start + query.len();
        spans.push(Span::styled(
            &line[start..end],
            Style::default().fg(Color::Black).bg(Color::Yellow),
        ));
        last_end = end;
    }

    if last_end < line.len() {
        spans.push(Span::raw(&line[last_end..]));
    }

    Line::from(spans)
}
