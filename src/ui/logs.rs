use std::borrow::Cow;
use std::sync::LazyLock;

use chrono::{DateTime, Local, NaiveDateTime, Utc};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use regex::Regex;
use serde_json::Value;

use crate::app::{App, Focus, InputMode, TimestampMode};
use crate::ui::theme::Theme;

/// Matches ISO 8601 / RFC 3339 timestamps at the start of a log line.
/// Covers K8s native format (`2024-01-15T10:00:00Z`, `…T10:00:00.123456789Z`)
/// and common application formats (`2024-01-15 10:00:00`, `…10:00:00,123`).
pub(crate) static TIMESTAMP_RE: LazyLock<Regex> = LazyLock::new(|| {
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
    // Detect optional K8s timestamp prefix (from LogParams.timestamps=true).
    // Lines may look like: `2026-02-24T16:36:51.600Z {"time":"...","msg":"hello"}`
    let (ts_prefix, json_part) = match TIMESTAMP_RE.find(line) {
        Some(m) if line[m.end()..].starts_with('{') => (&line[..m.end()], &line[m.end()..]),
        _ if line.starts_with('{') => ("", line),
        _ => return Cow::Borrowed(line),
    };

    let obj = match serde_json::from_str::<Value>(json_part) {
        Ok(Value::Object(map)) => map,
        _ => return Cow::Borrowed(line),
    };

    let mut parts: Vec<String> = Vec::new();
    let mut used_keys: Vec<&str> = Vec::new();

    // 1. Extract timestamp: prefer K8s prefix when present
    if !ts_prefix.is_empty() {
        parts.push(ts_prefix.trim_end().to_string());
        // Mark JSON time fields as used so they don't appear as key=value
        for &key in TIME_KEYS {
            if obj.contains_key(key) {
                used_keys.push(key);
            }
        }
    } else {
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

/// Strip K8s-prepended timestamp when the remainder has its own timestamp.
/// This prevents showing two timestamps (K8s + application) on the same line.
/// Lines are kept as-is when the remainder does not start with a recognisable
/// timestamp (e.g. plain text or JSON with `{`).
fn strip_duplicate_timestamp(line: &str) -> &str {
    if let Some(m) = TIMESTAMP_RE.find(line) {
        let rest = &line[m.end()..];
        if TIMESTAMP_RE.is_match(rest) {
            return rest;
        }
    }
    line
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
            let line = strip_duplicate_timestamp(line);
            if app.json_mode {
                format_json_line(line)
            } else {
                Cow::Borrowed(line)
            }
        })
        .collect();

    let visible_lines: Vec<Line> = formatted
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let s: &str = line.as_ref();
            let mut styled = if !app.search_query.is_empty() {
                highlight_search(s, &app.search_query, theme, app.timestamp_mode)
            } else {
                Line::from(colorize_log_line(s, theme, app.timestamp_mode))
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

// ---------------------------------------------------------------------------
// Timestamp display helpers
// ---------------------------------------------------------------------------

/// Try to parse a timestamp string into a `DateTime<Utc>`.
/// Handles RFC 3339 (with timezone) and `YYYY-MM-DD HH:MM:SS` (assumed UTC).
pub(crate) fn parse_log_timestamp(ts: &str) -> Option<DateTime<Utc>> {
    let trimmed = ts.trim();
    // Try RFC 3339 first (covers `2026-02-24T16:36:51.600Z`, `...+05:30`)
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.to_utc());
    }
    // Fall back to space-separated `YYYY-MM-DD HH:MM:SS` (assume UTC)
    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return Some(naive.and_utc());
    }
    None
}

/// Format a UTC timestamp as local time.
fn format_local(dt: DateTime<Utc>) -> String {
    let local: DateTime<Local> = dt.into();
    format!("{} ", local.format("%Y-%m-%d %H:%M:%S"))
}

/// Format a duration as a compact relative string.
fn format_relative(dt: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = now.signed_duration_since(dt).num_seconds().max(0);
    match secs {
        s if s < 60 => format!("{s}s ago "),
        s if s < 3600 => format!("{}m ago ", s / 60),
        s if s < 86400 => format!("{}h ago ", s / 3600),
        s => format!("{}d ago ", s / 86400),
    }
}

/// Convert a matched timestamp string according to the display mode.
/// Returns `None` if the timestamp cannot be parsed (caller keeps original).
fn convert_timestamp(ts: &str, mode: TimestampMode) -> Option<String> {
    match mode {
        TimestampMode::Utc => None, // keep original
        TimestampMode::Local => parse_log_timestamp(ts).map(format_local),
        TimestampMode::Relative => {
            parse_log_timestamp(ts).map(|dt| format_relative(dt, Utc::now()))
        }
    }
}

fn colorize_log_line<'a>(
    line: &'a str,
    theme: &Theme,
    timestamp_mode: TimestampMode,
) -> Vec<Span<'a>> {
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
        let ts_span = match convert_timestamp(ts, timestamp_mode) {
            Some(converted) => Span::styled(converted, Style::default().fg(theme.muted)),
            None => Span::styled(ts, Style::default().fg(theme.muted)),
        };
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

fn highlight_search(
    line: &str,
    query: &str,
    theme: &Theme,
    timestamp_mode: TimestampMode,
) -> Line<'static> {
    // Apply timestamp conversion so search operates on what the user sees
    let display: String = match TIMESTAMP_RE.find(line) {
        Some(m) => match convert_timestamp(&line[..m.end()], timestamp_mode) {
            Some(converted) => format!("{}{}", converted, &line[m.end()..]),
            None => line.to_string(),
        },
        None => line.to_string(),
    };

    let lower_line = display.to_lowercase();
    let lower_query = query.to_lowercase();

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut last_end = 0;

    for (start, _) in lower_line.match_indices(&lower_query) {
        if start > last_end {
            spans.push(Span::raw(display[last_end..start].to_string()));
        }
        let end = start + query.len();
        spans.push(Span::styled(
            display[start..end].to_string(),
            Style::default()
                .fg(theme.search_match_fg)
                .bg(theme.search_match_bg),
        ));
        last_end = end;
    }

    if last_end < display.len() {
        spans.push(Span::raw(display[last_end..].to_string()));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::DARK;
    use chrono::Timelike;

    // -- colorize_log_line --------------------------------------------------

    #[test]
    fn test_colorize_error_line_with_timestamp() {
        let spans = colorize_log_line(
            "2024-01-01 10:00:00 ERROR something broke",
            &DARK,
            TimestampMode::Utc,
        );
        assert_eq!(spans.len(), 2);
        // Timestamp in muted
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
        // Rest in error color
        assert_eq!(spans[1].style.fg, Some(DARK.log_error));
        assert!(spans[1].content.contains("ERROR"));
    }

    #[test]
    fn test_colorize_warn_line_with_timestamp() {
        let spans = colorize_log_line(
            "2024-01-01 10:00:00 WARN low memory",
            &DARK,
            TimestampMode::Utc,
        );
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
        assert_eq!(spans[1].style.fg, Some(DARK.log_warn));
    }

    #[test]
    fn test_colorize_debug_line_no_timestamp() {
        let spans = colorize_log_line("DEBUG: detailed trace info", &DARK, TimestampMode::Utc);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(DARK.log_debug));
    }

    #[test]
    fn test_colorize_info_line_no_timestamp() {
        let spans = colorize_log_line(
            "INFO: server started on port 8080",
            &DARK,
            TimestampMode::Utc,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, None);
    }

    #[test]
    fn test_colorize_lowercase_error_no_timestamp() {
        let spans = colorize_log_line("an error occurred in module", &DARK, TimestampMode::Utc);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(DARK.log_error));
    }

    #[test]
    fn test_timestamp_rfc3339_z() {
        let spans = colorize_log_line(
            "2024-01-15T10:00:00Z INFO started",
            &DARK,
            TimestampMode::Utc,
        );
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "2024-01-15T10:00:00Z ");
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
        // "INFO started" — no level keyword match (INFO not handled as special)
        assert_eq!(spans[1].style.fg, None);
    }

    #[test]
    fn test_timestamp_rfc3339_fractional() {
        let spans = colorize_log_line(
            "2024-01-15T10:00:00.123456789Z ERROR fail",
            &DARK,
            TimestampMode::Utc,
        );
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "2024-01-15T10:00:00.123456789Z ");
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
        assert_eq!(spans[1].style.fg, Some(DARK.log_error));
    }

    #[test]
    fn test_timestamp_space_separated() {
        let spans = colorize_log_line(
            "2024-01-15 10:00:00 request handled",
            &DARK,
            TimestampMode::Utc,
        );
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "2024-01-15 10:00:00 ");
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
    }

    #[test]
    fn test_timestamp_with_offset() {
        let spans = colorize_log_line(
            "2024-01-15T10:00:00+05:30 WARN slow",
            &DARK,
            TimestampMode::Utc,
        );
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "2024-01-15T10:00:00+05:30 ");
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
        assert_eq!(spans[1].style.fg, Some(DARK.log_warn));
    }

    #[test]
    fn test_no_timestamp_line() {
        let spans = colorize_log_line("[ERROR] connection refused", &DARK, TimestampMode::Utc);
        // No timestamp detected, entire line styled as error
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(DARK.log_error));
    }

    // -- highlight_search ---------------------------------------------------

    #[test]
    fn test_highlight_single_match() {
        let line = highlight_search("hello world", "world", &DARK, TimestampMode::Utc);
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content, "hello ");
        assert_eq!(line.spans[1].content, "world");
        assert_eq!(line.spans[1].style.bg, Some(DARK.search_match_bg));
        assert_eq!(line.spans[1].style.fg, Some(DARK.search_match_fg));
    }

    #[test]
    fn test_highlight_multiple_matches() {
        let line = highlight_search("error in error handler", "error", &DARK, TimestampMode::Utc);
        let texts: Vec<&str> = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, vec!["error", " in ", "error", " handler"]);
    }

    #[test]
    fn test_highlight_case_insensitive() {
        let line = highlight_search("ERROR: fatal", "error", &DARK, TimestampMode::Utc);
        assert_eq!(line.spans[0].content, "ERROR");
        assert_eq!(line.spans[0].style.bg, Some(DARK.search_match_bg));
    }

    #[test]
    fn test_highlight_no_match() {
        let line = highlight_search("hello world", "xyz", &DARK, TimestampMode::Utc);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "hello world");
        assert_eq!(line.spans[0].style.bg, None);
    }

    #[test]
    fn test_highlight_match_at_start() {
        let line = highlight_search("abc def", "abc", &DARK, TimestampMode::Utc);
        let texts: Vec<&str> = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, vec!["abc", " def"]);
        assert_eq!(line.spans[0].style.bg, Some(DARK.search_match_bg));
    }

    #[test]
    fn test_highlight_match_at_end() {
        let line = highlight_search("foo bar", "bar", &DARK, TimestampMode::Utc);
        let texts: Vec<&str> = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, vec!["foo ", "bar"]);
        assert_eq!(line.spans[1].style.bg, Some(DARK.search_match_bg));
    }

    #[test]
    fn test_highlight_entire_line() {
        let line = highlight_search("test", "test", &DARK, TimestampMode::Utc);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "test");
        assert_eq!(line.spans[0].style.bg, Some(DARK.search_match_bg));
    }

    #[test]
    fn test_highlight_converts_timestamp_in_relative_mode() {
        let line = highlight_search(
            "2026-02-24T16:36:51Z ERROR fail",
            "ago",
            &DARK,
            TimestampMode::Relative,
        );
        // Timestamp should be converted to relative, and "ago" should match
        let full_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(full_text.contains("ago"));
        assert!(
            line.spans
                .iter()
                .any(|s| s.style.bg == Some(DARK.search_match_bg))
        );
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
        let spans = colorize_log_line(result.as_ref(), &DARK, TimestampMode::Utc);
        // Should detect ERROR and color it
        let has_error_color = spans.iter().any(|s| s.style.fg == Some(DARK.log_error));
        assert!(has_error_color);
    }

    #[test]
    fn test_json_with_k8s_timestamp_prefix() {
        // When LogParams.timestamps=true, K8s prepends a timestamp before JSON
        let line = r#"2026-02-24T16:36:51.600Z {"time":"2026-02-24T16:36:51.600Z","level":"error","msg":"fail","status":500}"#;
        let result = format_json_line(line);
        // K8s timestamp should be used (not duplicated from JSON)
        assert!(result.starts_with("2026-02-24T16:36:51.600Z"));
        assert!(result.contains("[ERROR]"));
        assert!(result.contains("fail"));
        assert!(result.contains("status=500"));
        // JSON time field should NOT appear as key=value
        assert!(!result.contains("time="));
    }

    #[test]
    fn test_json_with_k8s_prefix_non_json_remainder() {
        // K8s timestamp prefix followed by plain text (not JSON)
        let line = "2026-02-24T16:36:51.600Z plain text log line";
        let result = format_json_line(line);
        // Should be returned as-is (no JSON flattening)
        assert_eq!(result.as_ref(), line);
    }

    // -- parse_log_timestamp ------------------------------------------------

    #[test]
    fn test_parse_rfc3339_z() {
        let dt = parse_log_timestamp("2026-02-24T16:36:51Z").unwrap();
        assert_eq!(dt.timestamp(), 1771951011);
    }

    #[test]
    fn test_parse_rfc3339_fractional() {
        let dt = parse_log_timestamp("2026-02-24T16:36:51.600Z").unwrap();
        assert_eq!(dt.timestamp(), 1771951011);
    }

    #[test]
    fn test_parse_rfc3339_offset() {
        let dt = parse_log_timestamp("2026-02-24T16:36:51+05:30").unwrap();
        // 16:36:51 +05:30 = 11:06:51 UTC
        assert_eq!(dt.hour(), 11);
    }

    #[test]
    fn test_parse_space_separated() {
        let dt = parse_log_timestamp("2026-02-24 16:36:51").unwrap();
        assert_eq!(dt.timestamp(), 1771951011);
    }

    #[test]
    fn test_parse_invalid_returns_none() {
        assert!(parse_log_timestamp("not-a-timestamp").is_none());
        assert!(parse_log_timestamp("").is_none());
    }

    // -- format_local -------------------------------------------------------

    #[test]
    fn test_format_local_contains_date() {
        let dt = parse_log_timestamp("2026-02-24T16:36:51Z").unwrap();
        let result = format_local(dt);
        // Must contain the date in some local representation
        assert!(result.contains("2026-02-24") || result.contains("2026-02-25"));
        assert!(result.ends_with(' '));
    }

    // -- format_relative ----------------------------------------------------

    #[test]
    fn test_format_relative_seconds() {
        let now = Utc::now();
        let dt = now - chrono::Duration::seconds(30);
        let result = format_relative(dt, now);
        assert_eq!(result, "30s ago ");
    }

    #[test]
    fn test_format_relative_minutes() {
        let now = Utc::now();
        let dt = now - chrono::Duration::seconds(300);
        let result = format_relative(dt, now);
        assert_eq!(result, "5m ago ");
    }

    #[test]
    fn test_format_relative_hours() {
        let now = Utc::now();
        let dt = now - chrono::Duration::seconds(7200);
        let result = format_relative(dt, now);
        assert_eq!(result, "2h ago ");
    }

    #[test]
    fn test_format_relative_days() {
        let now = Utc::now();
        let dt = now - chrono::Duration::seconds(172800);
        let result = format_relative(dt, now);
        assert_eq!(result, "2d ago ");
    }

    // -- convert_timestamp --------------------------------------------------

    #[test]
    fn test_convert_timestamp_utc_returns_none() {
        assert!(convert_timestamp("2026-02-24T16:36:51Z", TimestampMode::Utc).is_none());
    }

    #[test]
    fn test_convert_timestamp_local_returns_some() {
        let result = convert_timestamp("2026-02-24T16:36:51Z", TimestampMode::Local);
        assert!(result.is_some());
        assert!(result.unwrap().contains("2026-02-2"));
    }

    #[test]
    fn test_convert_timestamp_relative_returns_some() {
        let result = convert_timestamp("2026-02-24T16:36:51Z", TimestampMode::Relative);
        assert!(result.is_some());
        assert!(result.unwrap().contains("ago"));
    }

    #[test]
    fn test_convert_timestamp_invalid_returns_none() {
        assert!(convert_timestamp("not-a-ts", TimestampMode::Local).is_none());
        assert!(convert_timestamp("not-a-ts", TimestampMode::Relative).is_none());
    }

    // -- colorize_log_line with timestamp modes -----------------------------

    #[test]
    fn test_colorize_local_mode_converts_timestamp() {
        let spans = colorize_log_line(
            "2026-02-24T16:36:51Z ERROR fail",
            &DARK,
            TimestampMode::Local,
        );
        assert_eq!(spans.len(), 2);
        // Timestamp should be converted (owned String, not the original)
        assert!(spans[0].content.contains("2026-02-2"));
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
    }

    #[test]
    fn test_colorize_relative_mode_converts_timestamp() {
        let spans = colorize_log_line(
            "2026-02-24T16:36:51Z ERROR fail",
            &DARK,
            TimestampMode::Relative,
        );
        assert_eq!(spans.len(), 2);
        assert!(spans[0].content.contains("ago"));
        assert_eq!(spans[0].style.fg, Some(DARK.muted));
    }

    #[test]
    fn test_colorize_utc_mode_keeps_original() {
        let spans = colorize_log_line("2026-02-24T16:36:51Z ERROR fail", &DARK, TimestampMode::Utc);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "2026-02-24T16:36:51Z ");
    }

    // -- strip_duplicate_timestamp -------------------------------------------

    #[test]
    fn test_strip_dup_ts_removes_k8s_prefix_when_app_has_own_timestamp() {
        // K8s prefix + application timestamp → strip K8s prefix
        let line = "2026-02-24T16:36:51.600000000Z 2026-02-24T16:36:51.600Z [ERROR] fail";
        let result = strip_duplicate_timestamp(line);
        assert_eq!(result, "2026-02-24T16:36:51.600Z [ERROR] fail");
    }

    #[test]
    fn test_strip_dup_ts_keeps_line_when_no_app_timestamp() {
        // K8s prefix + plain text (no second timestamp) → keep as-is
        let line = "2026-02-24T16:36:51.600000000Z plain text log message";
        let result = strip_duplicate_timestamp(line);
        assert_eq!(result, line);
    }

    #[test]
    fn test_strip_dup_ts_keeps_line_when_json_remainder() {
        // K8s prefix + JSON object → keep as-is (JSON flattening handles it)
        let line = r#"2026-02-24T16:36:51.600Z {"time":"2026-02-24T16:36:51.600Z","msg":"hello"}"#;
        let result = strip_duplicate_timestamp(line);
        assert_eq!(result, line);
    }

    #[test]
    fn test_strip_dup_ts_keeps_line_when_no_timestamp_at_all() {
        // No timestamp at start → keep as-is
        let line = "[ERROR] connection refused";
        let result = strip_duplicate_timestamp(line);
        assert_eq!(result, line);
    }

    #[test]
    fn test_strip_dup_ts_with_space_separated_timestamps() {
        // K8s prefix (RFC 3339) + space-separated app timestamp
        let line = "2026-02-24T16:36:51Z 2026-02-24 16:36:51 INFO started";
        let result = strip_duplicate_timestamp(line);
        assert_eq!(result, "2026-02-24 16:36:51 INFO started");
    }

    #[test]
    fn test_strip_dup_ts_single_timestamp_preserved() {
        // Only one timestamp, no duplicate → keep as-is
        let line = "2026-02-24T16:36:51Z ERROR something broke";
        let result = strip_duplicate_timestamp(line);
        assert_eq!(result, line);
    }
}
