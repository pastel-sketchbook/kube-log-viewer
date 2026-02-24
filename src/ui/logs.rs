use std::borrow::Cow;
use std::sync::LazyLock;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use regex::Regex;
use serde_json::Value;

use crate::app::{App, Focus, InputMode};
use crate::ui::theme::Theme;

/// Matches ISO 8601 / RFC 3339 timestamps at the start of a log line.
/// Covers K8s native format (`2024-01-15T10:00:00Z`, `…T10:00:00.123456789Z`)
/// and common application formats (`2024-01-15 10:00:00`, `…10:00:00,123`).
static TIMESTAMP_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Safety: this is a hardcoded literal that is guaranteed to compile.
    Regex::new(r"^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}([.,]\d+)?(Z|[+-]\d{2}:?\d{2})?\s*")
        .expect("hardcoded timestamp regex is valid")
});

// ---------------------------------------------------------------------------
// JSON flattening
// ---------------------------------------------------------------------------

/// Well-known field names for timestamp extraction.
const TIME_KEYS: &[&str] = &["time", "timestamp", "ts", "@timestamp", "datetime"];

/// Well-known field names for log-level extraction.
const LEVEL_KEYS: &[&str] = &["level", "severity", "loglevel", "log_level", "lvl"];

/// Well-known field names for message extraction.
const MSG_KEYS: &[&str] = &["msg", "message"];

/// Flatten a JSON object line into `timestamp [LEVEL] message key=value ...`.
///
/// Returns `Cow::Borrowed` for non-JSON lines (no allocation).
/// The output is designed to feed into `colorize_log_line()` so that
/// the extracted timestamp gets muted color and the level gets keyword color.
fn format_json_line(line: &str) -> Cow<'_, str> {
    if !line.starts_with('{') {
        return Cow::Borrowed(line);
    }

    let obj = match serde_json::from_str::<Value>(line) {
        Ok(Value::Object(map)) => map,
        _ => return Cow::Borrowed(line),
    };

    let mut parts: Vec<String> = Vec::new();
    let mut used_keys: Vec<&str> = Vec::new();

    // 1. Extract timestamp
    for &key in TIME_KEYS {
        if let Some(val) = obj.get(key)
            && let Some(s) = val.as_str()
            && !s.is_empty()
        {
            parts.push(s.to_string());
            used_keys.push(key);
            break;
        }
    }

    // 2. Extract level
    for &key in LEVEL_KEYS {
        if let Some(val) = obj.get(key)
            && let Some(s) = val.as_str()
            && !s.is_empty()
        {
            parts.push(format!("[{}]", s.to_uppercase()));
            used_keys.push(key);
            break;
        }
    }

    // 3. Extract message
    for &key in MSG_KEYS {
        if let Some(val) = obj.get(key)
            && let Some(s) = val.as_str()
            && !s.is_empty()
        {
            parts.push(s.to_string());
            used_keys.push(key);
            break;
        }
    }

    // 4. Remaining fields as key=value
    for (key, val) in &obj {
        if used_keys.contains(&key.as_str()) {
            continue;
        }
        match val {
            Value::Null => continue,
            Value::String(s) if s.is_empty() => continue,
            Value::String(s) => parts.push(format!("{key}={s}")),
            Value::Number(n) => parts.push(format!("{key}={n}")),
            Value::Bool(b) => parts.push(format!("{key}={b}")),
            _ => parts.push(format!("{key}={val}")),
        }
    }

    Cow::Owned(parts.join(" "))
}

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

    // Apply JSON flattening (if enabled) before colorization.
    // Formatted strings must outlive the Span references, so collect first.
    let formatted: Vec<Cow<str>> = filtered_lines
        .iter()
        .skip(scroll_offset)
        .take(inner_height)
        .map(|line| {
            if app.json_mode {
                format_json_line(line)
            } else {
                Cow::Borrowed(*line)
            }
        })
        .collect();

    let visible_lines: Vec<Line> = formatted
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let s: &str = line.as_ref();
            let mut styled = if !app.search_query.is_empty() {
                highlight_search(s, &app.search_query, theme)
            } else {
                Line::from(colorize_log_line(s, theme))
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
    let level_color = if line.contains("ERROR") || line.contains("error") {
        Some(theme.log_error)
    } else if line.contains("WARN") || line.contains("warn") {
        Some(theme.log_warn)
    } else if line.contains("DEBUG") || line.contains("debug") {
        Some(theme.log_debug)
    } else {
        None
    };

    if let Some(m) = TIMESTAMP_RE.find(line) {
        let ts = &line[..m.end()];
        let rest = &line[m.end()..];
        let ts_span = Span::styled(ts, Style::default().fg(theme.muted));
        let rest_span = match level_color {
            Some(color) => Span::styled(rest, Style::default().fg(color)),
            None => Span::raw(rest),
        };
        vec![ts_span, rest_span]
    } else {
        match level_color {
            Some(color) => vec![Span::styled(line, Style::default().fg(color))],
            None => vec![Span::raw(line)],
        }
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
    fn test_colorize_error_line_with_timestamp() {
        let spans = colorize_log_line("2024-01-01 10:00:00 ERROR something broke", &DARK);
        assert_eq!(spans.len(), 2);
        // Timestamp in muted
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
        // Rest in error color
        assert_eq!(spans[1].style.fg, Some(DARK.log_error));
        assert!(spans[1].content.contains("ERROR"));
    }

    #[test]
    fn test_colorize_warn_line_with_timestamp() {
        let spans = colorize_log_line("2024-01-01 10:00:00 WARN low memory", &DARK);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
        assert_eq!(spans[1].style.fg, Some(DARK.log_warn));
    }

    #[test]
    fn test_colorize_debug_line_no_timestamp() {
        let spans = colorize_log_line("DEBUG: detailed trace info", &DARK);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(DARK.log_debug));
    }

    #[test]
    fn test_colorize_info_line_no_timestamp() {
        let spans = colorize_log_line("INFO: server started on port 8080", &DARK);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, None);
    }

    #[test]
    fn test_colorize_lowercase_error_no_timestamp() {
        let spans = colorize_log_line("an error occurred in module", &DARK);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(DARK.log_error));
    }

    #[test]
    fn test_timestamp_rfc3339_z() {
        let spans = colorize_log_line("2024-01-15T10:00:00Z INFO started", &DARK);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "2024-01-15T10:00:00Z ");
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
        // "INFO started" — no level keyword match (INFO not handled as special)
        assert_eq!(spans[1].style.fg, None);
    }

    #[test]
    fn test_timestamp_rfc3339_fractional() {
        let spans = colorize_log_line("2024-01-15T10:00:00.123456789Z ERROR fail", &DARK);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "2024-01-15T10:00:00.123456789Z ");
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
        assert_eq!(spans[1].style.fg, Some(DARK.log_error));
    }

    #[test]
    fn test_timestamp_space_separated() {
        let spans = colorize_log_line("2024-01-15 10:00:00 request handled", &DARK);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "2024-01-15 10:00:00 ");
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
    }

    #[test]
    fn test_timestamp_with_offset() {
        let spans = colorize_log_line("2024-01-15T10:00:00+05:30 WARN slow", &DARK);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "2024-01-15T10:00:00+05:30 ");
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
        assert_eq!(spans[1].style.fg, Some(DARK.log_warn));
    }

    #[test]
    fn test_no_timestamp_line() {
        let spans = colorize_log_line("[ERROR] connection refused", &DARK);
        // No timestamp detected, entire line styled as error
        assert_eq!(spans.len(), 1);
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

    // -- format_json_line ---------------------------------------------------

    #[test]
    fn test_json_non_json_passthrough() {
        let line = "plain log line";
        let result = format_json_line(line);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result.as_ref(), "plain log line");
    }

    #[test]
    fn test_json_invalid_json_passthrough() {
        let line = "{not valid json at all";
        let result = format_json_line(line);
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    #[test]
    fn test_json_array_passthrough() {
        let line = r#"[1, 2, 3]"#;
        let result = format_json_line(line);
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    #[test]
    fn test_json_extracts_time_field() {
        let line = r#"{"time":"2026-02-24T16:36:51.600Z","method":"GET","uri":"/metrics"}"#;
        let result = format_json_line(line);
        assert!(result.starts_with("2026-02-24T16:36:51.600Z"));
        assert!(result.contains("method=GET"));
        assert!(result.contains("uri=/metrics"));
        // time should not appear as key=value
        assert!(!result.contains("time="));
    }

    #[test]
    fn test_json_extracts_timestamp_field() {
        let line = r#"{"timestamp":"2026-01-01T00:00:00Z","msg":"hello"}"#;
        let result = format_json_line(line);
        assert!(result.starts_with("2026-01-01T00:00:00Z"));
        assert!(result.contains("hello"));
        assert!(!result.contains("timestamp="));
        assert!(!result.contains("msg="));
    }

    #[test]
    fn test_json_extracts_level_as_bracket() {
        let line = r#"{"level":"error","msg":"something broke"}"#;
        let result = format_json_line(line);
        assert!(result.contains("[ERROR]"));
        assert!(result.contains("something broke"));
        assert!(!result.contains("level="));
    }

    #[test]
    fn test_json_extracts_severity() {
        let line = r#"{"severity":"warn","message":"disk full"}"#;
        let result = format_json_line(line);
        assert!(result.contains("[WARN]"));
        assert!(result.contains("disk full"));
    }

    #[test]
    fn test_json_skips_empty_strings_and_null() {
        let line = r#"{"time":"2026-01-01T00:00:00Z","error":"","extra":null,"status":200}"#;
        let result = format_json_line(line);
        assert!(!result.contains("error="));
        assert!(!result.contains("extra="));
        assert!(result.contains("status=200"));
    }

    #[test]
    fn test_json_formats_booleans() {
        let line = r#"{"ok":true,"retry":false}"#;
        let result = format_json_line(line);
        assert!(result.contains("ok=true"));
        assert!(result.contains("retry=false"));
    }

    #[test]
    fn test_json_full_http_access_log() {
        let line = r#"{"time":"2026-02-24T16:36:51.600Z","id":"abc-123","remote_ip":"10.0.0.1","method":"GET","uri":"/health","status":200,"error":"","latency_human":"1.5ms","bytes_in":0,"bytes_out":19}"#;
        let result = format_json_line(line);
        // Timestamp first
        assert!(result.starts_with("2026-02-24T16:36:51.600Z"));
        // Key fields present
        assert!(result.contains("method=GET"));
        assert!(result.contains("uri=/health"));
        assert!(result.contains("status=200"));
        assert!(result.contains("latency_human=1.5ms"));
        // Empty error skipped
        assert!(!result.contains("error="));
    }

    #[test]
    fn test_json_timestamp_feeds_colorizer() {
        // Verify the flattened output starts with a timestamp that TIMESTAMP_RE can match
        let line = r#"{"time":"2026-02-24T16:36:51.600Z","status":200}"#;
        let result = format_json_line(line);
        assert!(TIMESTAMP_RE.is_match(result.as_ref()));
    }

    #[test]
    fn test_json_level_feeds_colorizer() {
        // Verify the flattened output contains ERROR keyword for colorize_log_line
        let line = r#"{"level":"error","msg":"fail"}"#;
        let result = format_json_line(line);
        let spans = colorize_log_line(result.as_ref(), &DARK);
        // Should detect ERROR and color it
        let has_error_color = spans.iter().any(|s| s.style.fg == Some(DARK.log_error));
        assert!(has_error_color);
    }
}
