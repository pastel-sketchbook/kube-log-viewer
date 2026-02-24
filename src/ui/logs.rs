use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, Focus, InputMode};
use crate::ui::theme::Theme;

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

    let theme = app.theme();
    let border_color = match app.focus {
        Focus::Logs => theme.border_focused,
        _ => theme.border_unfocused,
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
        .enumerate()
        .map(|(i, line)| {
            let mut styled = if !app.search_query.is_empty() {
                highlight_search(line, &app.search_query, theme)
            } else {
                Line::from(colorize_log_line(line, theme))
            };
            if i % 2 == 1 {
                styled = styled.style(Style::default().bg(theme.zebra_bg));
            }
            styled
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
    let theme = app.theme();
    let input_area = Rect {
        x: area.x + 1,
        y: area.y + area.height.saturating_sub(2),
        width: area.width.saturating_sub(2),
        height: 1,
    };

    let input = Paragraph::new(format!("/{}", app.search_query)).style(
        Style::default()
            .fg(theme.search_fg)
            .bg(theme.search_input_bg),
    );

    frame.render_widget(input, input_area);
}

fn colorize_log_line<'a>(line: &'a str, theme: &Theme) -> Vec<Span<'a>> {
    if line.contains("ERROR") || line.contains("error") {
        vec![Span::styled(line, Style::default().fg(theme.log_error))]
    } else if line.contains("WARN") || line.contains("warn") {
        vec![Span::styled(line, Style::default().fg(theme.log_warn))]
    } else if line.contains("DEBUG") || line.contains("debug") {
        vec![Span::styled(line, Style::default().fg(theme.log_debug))]
    } else {
        vec![Span::raw(line)]
    }
}

fn highlight_search<'a>(line: &'a str, query: &str, theme: &Theme) -> Line<'a> {
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
            Style::default()
                .fg(theme.search_match_fg)
                .bg(theme.search_match_bg),
        ));
        last_end = end;
    }

    if last_end < line.len() {
        spans.push(Span::raw(&line[last_end..]));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::DARK;

    // -- colorize_log_line --------------------------------------------------

    #[test]
    fn test_colorize_error_line() {
        let spans = colorize_log_line("2024-01-01 ERROR something broke", &DARK);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(DARK.log_error));
    }

    #[test]
    fn test_colorize_warn_line() {
        let spans = colorize_log_line("2024-01-01 WARN low memory", &DARK);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(DARK.log_warn));
    }

    #[test]
    fn test_colorize_debug_line() {
        let spans = colorize_log_line("DEBUG: detailed trace info", &DARK);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(DARK.log_debug));
    }

    #[test]
    fn test_colorize_info_line_is_unstyled() {
        let spans = colorize_log_line("INFO: server started on port 8080", &DARK);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, None);
    }

    #[test]
    fn test_colorize_lowercase_error() {
        let spans = colorize_log_line("an error occurred in module", &DARK);
        assert_eq!(spans[0].style.fg, Some(DARK.log_error));
    }

    // -- highlight_search ---------------------------------------------------

    #[test]
    fn test_highlight_single_match() {
        let line = highlight_search("hello world", "world", &DARK);
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content, "hello ");
        assert_eq!(line.spans[1].content, "world");
        assert_eq!(line.spans[1].style.bg, Some(DARK.search_match_bg));
        assert_eq!(line.spans[1].style.fg, Some(DARK.search_match_fg));
    }

    #[test]
    fn test_highlight_multiple_matches() {
        let line = highlight_search("error in error handler", "error", &DARK);
        let texts: Vec<&str> = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, vec!["error", " in ", "error", " handler"]);
    }

    #[test]
    fn test_highlight_case_insensitive() {
        let line = highlight_search("ERROR: fatal", "error", &DARK);
        assert_eq!(line.spans[0].content, "ERROR");
        assert_eq!(line.spans[0].style.bg, Some(DARK.search_match_bg));
    }

    #[test]
    fn test_highlight_no_match() {
        let line = highlight_search("hello world", "xyz", &DARK);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "hello world");
        assert_eq!(line.spans[0].style.bg, None);
    }

    #[test]
    fn test_highlight_match_at_start() {
        let line = highlight_search("abc def", "abc", &DARK);
        let texts: Vec<&str> = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, vec!["abc", " def"]);
        assert_eq!(line.spans[0].style.bg, Some(DARK.search_match_bg));
    }

    #[test]
    fn test_highlight_match_at_end() {
        let line = highlight_search("foo bar", "bar", &DARK);
        let texts: Vec<&str> = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, vec!["foo ", "bar"]);
        assert_eq!(line.spans[1].style.bg, Some(DARK.search_match_bg));
    }

    #[test]
    fn test_highlight_entire_line() {
        let line = highlight_search("test", "test", &DARK);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "test");
        assert_eq!(line.spans[0].style.bg, Some(DARK.search_match_bg));
    }
}
