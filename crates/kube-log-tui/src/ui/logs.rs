use std::borrow::Cow;

use jiff::{Timestamp, tz::TimeZone};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, Focus, InputMode, MAX_STREAMS, StreamMode, TimestampMode};
use crate::ui::theme::Theme;
use kube_log_core::parse::{
    TIMESTAMP_RE, format_json_line, parse_log_timestamp, strip_duplicate_timestamp,
};

/// Stream colors for multi-stream mode pod tags.
const STREAM_COLORS: &[Color] = &[
    Color::Cyan,
    Color::Yellow,
    Color::Magenta,
    Color::Green,
    Color::Red,
    Color::Blue,
];

/// Get the color for a stream source based on its index in the streams list.
fn stream_color(app: &App, source: &str) -> Color {
    let idx = app
        .streams
        .iter()
        .position(|h| h.pod_name == source)
        .unwrap_or(0);
    STREAM_COLORS[idx % STREAM_COLORS.len()]
}

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    if app.stream_mode == StreamMode::Split && app.streams.len() >= 2 {
        render_split(frame, app, area);
    } else {
        render_single_or_merged(frame, app, area);
    }
}

fn render_single_or_merged(frame: &mut Frame, app: &App, area: Rect) {
    let title = match (&app.selected_pod, &app.selected_container) {
        (Some(pod), Some(container)) => format!(" Logs: {} / {} ", pod, container),
        (Some(pod), None) => format!(" Logs: {} ", pod),
        _ => " Logs ".to_string(),
    };

    let theme = app.theme();
    let border_color = match app.focus {
        Focus::Logs => theme.border_focused,
        _ => theme.border_unfocused,
    };

    let is_merged = app.stream_mode == StreamMode::Merged && app.streams.len() > 1;

    let mut title_spans: Vec<Span> = vec![Span::styled(title, Style::default().fg(theme.accent))];
    if is_merged {
        title_spans.push(Span::styled(
            " [MERGED] ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if app.follow_mode {
        title_spans.push(Span::styled(
            " [FOLLOW] ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if !app.search_query.is_empty() {
        title_spans.push(Span::styled(
            format!(" [/{}] ", app.search_query),
            Style::default().fg(theme.search_fg),
        ));
    }
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
    let formatted: Vec<(Option<&str>, Cow<str>)> = filtered_lines
        .iter()
        .skip(scroll_offset)
        .take(inner_height)
        .map(|tl| {
            let line = strip_duplicate_timestamp(&tl.line);
            let source = if is_merged && !tl.source.is_empty() {
                Some(tl.source.as_str())
            } else {
                None
            };
            let formatted = if app.json_mode {
                format_json_line(line)
            } else {
                Cow::Borrowed(line)
            };
            (source, formatted)
        })
        .collect();

    // Available text width inside the bordered block.
    let text_width = area.width.saturating_sub(2) as usize;

    let visible_lines: Vec<Line> = formatted
        .iter()
        .map(|(source, line)| {
            let s: &str = line.as_ref();
            let mut spans: Vec<Span> = Vec::new();

            // Prepend pod tag in merged mode
            if let Some(src) = source {
                let color = stream_color(app, src);
                // Middle-truncate long pod names to keep the meaningful prefix
                // and unique suffix (e.g. "my-deploy…-x9k2z" instead of
                // losing the prefix by taking last N chars).
                let tag: std::borrow::Cow<'_, str> = if src.len() > 20 {
                    let prefix_len = 10;
                    let suffix_len = 9; // 10 + 1 (ellipsis) + 9 = 20
                    let prefix = &src[..prefix_len];
                    let suffix = &src[src.len() - suffix_len..];
                    std::borrow::Cow::Owned(format!("{prefix}\u{2026}{suffix}"))
                } else {
                    std::borrow::Cow::Borrowed(src)
                };
                spans.push(Span::styled(
                    format!("[{tag}] "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ));
            }

            let line_spans = if !app.search_query.is_empty() {
                let hl = highlight_search(s, &app.search_query, theme, app.timestamp_mode);
                hl.spans
            } else {
                colorize_log_line(s, theme, app.timestamp_mode)
                    .into_iter()
                    .map(|sp| Span::from(sp.content.into_owned()).style(sp.style))
                    .collect()
            };
            spans.extend(line_spans);

            Line::from(spans)
        })
        .collect();

    // When wrapping is on, logical lines expand to multiple visual lines.
    // Calculate the overflow so we can scroll the Paragraph to keep the
    // bottom (most recent) lines visible instead of clipping them.
    let wrap_scroll_y = if app.wrap_lines {
        if text_width > 0 {
            let total_visual: usize = visible_lines
                .iter()
                .map(|l| {
                    let w = l.width();
                    if w == 0 { 1 } else { w.div_ceil(text_width) }
                })
                .sum();
            total_visual
                .saturating_sub(inner_height)
                .min(u16::MAX as usize) as u16
        } else {
            0
        }
    } else {
        0
    };

    // Build a map of visual row -> is_zebra before rendering.
    // We apply zebra bg directly to the buffer after rendering so it
    // covers the full row width regardless of wrapping.
    let zebra_rows = compute_zebra_rows(&visible_lines, text_width, app.wrap_lines, wrap_scroll_y);

    let block = Block::default()
        .title(Line::from(title_spans))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let mut paragraph = Paragraph::new(visible_lines).block(block);

    if app.wrap_lines {
        paragraph = paragraph.wrap(Wrap { trim: false });
        if wrap_scroll_y > 0 {
            paragraph = paragraph.scroll((wrap_scroll_y, 0));
        }
    }

    frame.render_widget(paragraph, area);

    // Paint zebra background on the buffer for full-width striping.
    apply_zebra_to_buffer(frame, area, &zebra_rows, theme.zebra_bg);

    // Render search input bar when in search mode
    if app.input_mode == InputMode::Search {
        render_search_input(frame, app, area);
    }
}

fn render_split(frame: &mut Frame, app: &App, area: Rect) {
    let n = app.streams.len().min(MAX_STREAMS);
    if n == 0 {
        return;
    }
    let pct = (100 / n) as u16;
    let n_u16 = n as u16; // n <= MAX_STREAMS (4), always fits u16
    let constraints: Vec<Constraint> = (0..n)
        .map(|i| {
            if i == n.saturating_sub(1) {
                // Last pane absorbs rounding remainder
                Constraint::Percentage(
                    100u16.saturating_sub(pct.saturating_mul(n_u16.saturating_sub(1))),
                )
            } else {
                Constraint::Percentage(pct)
            }
        })
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for i in 0..n {
        render_pane(frame, app, chunks[i], i);
    }
}

fn render_pane(frame: &mut Frame, app: &App, area: Rect, pane_idx: usize) {
    let theme = app.theme();
    let is_active = pane_idx == app.active_pane;

    let handle = match app.streams.get(pane_idx) {
        Some(h) => h,
        None => return,
    };

    let title = match &handle.container {
        Some(c) => format!(" [{}] {} / {} ", pane_idx + 1, handle.pod_name, c),
        None => format!(" [{}] {} ", pane_idx + 1, handle.pod_name),
    };

    let border_color = if is_active && app.focus == Focus::Logs {
        theme.border_focused
    } else {
        theme.border_unfocused
    };

    let mut title_spans: Vec<Span> = vec![Span::styled(title, Style::default().fg(theme.accent))];
    if handle.view.follow_mode {
        title_spans.push(Span::styled(
            " [FOLLOW] ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if is_active && !app.search_query.is_empty() {
        title_spans.push(Span::styled(
            format!(" [/{}] ", app.search_query),
            Style::default().fg(theme.search_fg),
        ));
    }
    let filtered_lines = app.filtered_log_lines_for_pane(pane_idx);
    let total_lines = filtered_lines.len();

    let inner_height = area.height.saturating_sub(2) as usize;
    let scroll_offset = if handle.view.follow_mode {
        total_lines.saturating_sub(inner_height)
    } else {
        handle
            .view
            .scroll_offset
            .min(total_lines.saturating_sub(inner_height))
    };

    let formatted: Vec<Cow<str>> = filtered_lines
        .iter()
        .skip(scroll_offset)
        .take(inner_height)
        .map(|tl| {
            let line = strip_duplicate_timestamp(&tl.line);
            if app.json_mode {
                format_json_line(line)
            } else {
                Cow::Borrowed(line)
            }
        })
        .collect();

    // Available text width inside the bordered block.
    let text_width = area.width.saturating_sub(2) as usize;

    let visible_lines: Vec<Line> = formatted
        .iter()
        .map(|line| {
            let s: &str = line.as_ref();
            let spans: Vec<Span> = if !app.search_query.is_empty() && is_active {
                highlight_search(s, &app.search_query, theme, app.timestamp_mode).spans
            } else {
                colorize_log_line(s, theme, app.timestamp_mode)
                    .into_iter()
                    .map(|sp| Span::from(sp.content.into_owned()).style(sp.style))
                    .collect()
            };

            Line::from(spans)
        })
        .collect();

    // Same wrap-scroll adjustment as render_single_or_merged: keep bottom
    // lines visible when wrapping causes visual overflow.
    let wrap_scroll_y = if app.wrap_lines {
        if text_width > 0 {
            let total_visual: usize = visible_lines
                .iter()
                .map(|l| {
                    let w = l.width();
                    if w == 0 { 1 } else { w.div_ceil(text_width) }
                })
                .sum();
            total_visual
                .saturating_sub(inner_height)
                .min(u16::MAX as usize) as u16
        } else {
            0
        }
    } else {
        0
    };

    let zebra_rows = compute_zebra_rows(&visible_lines, text_width, app.wrap_lines, wrap_scroll_y);

    let block = Block::default()
        .title(Line::from(title_spans))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let mut paragraph = Paragraph::new(visible_lines).block(block);

    if app.wrap_lines {
        paragraph = paragraph.wrap(Wrap { trim: false });
        if wrap_scroll_y > 0 {
            paragraph = paragraph.scroll((wrap_scroll_y, 0));
        }
    }

    frame.render_widget(paragraph, area);

    apply_zebra_to_buffer(frame, area, &zebra_rows, theme.zebra_bg);

    // Render search input in the active pane only
    if is_active && app.input_mode == InputMode::Search {
        render_search_input(frame, app, area);
    }
}

/// Compute which visual rows should have the zebra background.
/// Returns a Vec<bool> indexed by visual row within the inner area.
fn compute_zebra_rows(lines: &[Line], text_width: usize, wrap: bool, scroll_y: u16) -> Vec<bool> {
    let mut rows = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let is_zebra = i % 2 == 1;
        let visual_count = if wrap && text_width > 0 {
            let w = line.width();
            if w == 0 { 1 } else { w.div_ceil(text_width) }
        } else {
            1
        };
        for _ in 0..visual_count {
            rows.push(is_zebra);
        }
    }
    // When scrolled (wrap overflow), skip the first `scroll_y` visual rows.
    if scroll_y > 0 {
        rows.drain(..rows.len().min(scroll_y as usize));
    }
    rows
}

/// Paint zebra background directly on the frame buffer so it covers
/// the full row width, independent of Paragraph rendering.
fn apply_zebra_to_buffer(frame: &mut Frame, area: Rect, zebra_rows: &[bool], zebra_bg: Color) {
    // Inner area = area minus 1-cell border on each side.
    let inner_x = area.x + 1;
    let inner_y = area.y + 1;
    let inner_w = area.width.saturating_sub(2);
    let inner_h = area.height.saturating_sub(2);
    let buf = frame.buffer_mut();
    for row in 0..inner_h {
        if let Some(&true) = zebra_rows.get(row as usize) {
            for col in 0..inner_w {
                let cell = &mut buf[(inner_x + col, inner_y + row)];
                cell.set_bg(zebra_bg);
            }
        }
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

/// Format a UTC timestamp as local time.
fn format_local(ts: Timestamp) -> String {
    let local = ts.to_zoned(TimeZone::system());
    format!("{} ", local.strftime("%Y-%m-%d %H:%M:%S"))
}

/// Fixed display width for relative timestamps (right-aligned).
/// Keeps log content vertically aligned regardless of value.
const RELATIVE_WIDTH: usize = 8;

/// Format a duration as a compact relative string, right-aligned to a fixed
/// width so that log content after the timestamp starts at the same column.
fn format_relative(ts: Timestamp, now: Timestamp) -> String {
    let secs = now.duration_since(ts).as_secs().max(0);
    let rel = match secs {
        s if s < 60 => format!("{s}s ago"),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86400),
    };
    format!("{rel:>RELATIVE_WIDTH$} ")
}

/// Convert a matched timestamp string according to the display mode.
/// Returns `None` if the timestamp cannot be parsed (caller keeps original).
fn convert_timestamp(ts: &str, mode: TimestampMode) -> Option<String> {
    match mode {
        TimestampMode::Utc => None, // keep original
        TimestampMode::Local => parse_log_timestamp(ts).map(format_local),
        TimestampMode::Relative => {
            parse_log_timestamp(ts).map(|dt| format_relative(dt, Timestamp::now()))
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
            Some(converted) => Span::styled(converted, Style::default().fg(theme.log_timestamp)),
            None => Span::styled(ts, Style::default().fg(theme.log_timestamp)),
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
    // Apply timestamp conversion so search operates on what the user sees.
    // Also track where the timestamp ends so we can style it.
    let (display, ts_end): (String, usize) = match TIMESTAMP_RE.find(line) {
        Some(m) => match convert_timestamp(&line[..m.end()], timestamp_mode) {
            Some(converted) => {
                let clen = converted.len();
                (format!("{}{}", converted, &line[m.end()..]), clen)
            }
            None => (line.to_string(), m.end()),
        },
        None => (line.to_string(), 0),
    };

    let lower_line = display.to_lowercase();
    let lower_query = query.to_lowercase();

    let ts_style = Style::default().fg(theme.log_timestamp);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut last_end = 0;

    for (start, _) in lower_line.match_indices(&lower_query) {
        if start > last_end {
            push_non_match_spans(&mut spans, &display, last_end, start, ts_end, ts_style);
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
        push_non_match_spans(
            &mut spans,
            &display,
            last_end,
            display.len(),
            ts_end,
            ts_style,
        );
    }

    Line::from(spans)
}

/// Push non-match segments into `spans`, applying timestamp styling to the
/// portion that falls within `[0..ts_end)`.
fn push_non_match_spans(
    spans: &mut Vec<Span<'static>>,
    display: &str,
    seg_start: usize,
    seg_end: usize,
    ts_end: usize,
    ts_style: Style,
) {
    if seg_start >= seg_end {
        return;
    }
    if ts_end == 0 || seg_start >= ts_end {
        // Entirely past the timestamp
        spans.push(Span::raw(display[seg_start..seg_end].to_string()));
    } else if seg_end <= ts_end {
        // Entirely within the timestamp
        spans.push(Span::styled(
            display[seg_start..seg_end].to_string(),
            ts_style,
        ));
    } else {
        // Straddles the boundary
        spans.push(Span::styled(
            display[seg_start..ts_end].to_string(),
            ts_style,
        ));
        spans.push(Span::raw(display[ts_end..seg_end].to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::DARK;

    // -- colorize_log_line --------------------------------------------------

    #[test]
    fn test_colorize_error_line_with_timestamp() {
        let spans = colorize_log_line(
            "2024-01-01 10:00:00 ERROR something broke",
            &DARK,
            TimestampMode::Utc,
        );
        assert_eq!(spans.len(), 2);
        // Timestamp in log_timestamp color
        assert_eq!(spans[0].style.fg, Some(DARK.log_timestamp));
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
        assert_eq!(spans[0].style.fg, Some(DARK.log_timestamp));
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
        assert_eq!(spans[0].style.fg, Some(DARK.log_timestamp));
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
        assert_eq!(spans[0].style.fg, Some(DARK.log_timestamp));
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
        assert_eq!(spans[0].style.fg, Some(DARK.log_timestamp));
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
        assert_eq!(spans[0].style.fg, Some(DARK.log_timestamp));
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

    // -- format_json_line + colorize integration -----------------------------

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
        let now = Timestamp::now();
        let dt = now - jiff::SignedDuration::from_secs(30);
        let result = format_relative(dt, now);
        assert_eq!(result, " 30s ago ");
    }

    #[test]
    fn test_format_relative_minutes() {
        let now = Timestamp::now();
        let dt = now - jiff::SignedDuration::from_secs(300);
        let result = format_relative(dt, now);
        assert_eq!(result, "  5m ago ");
    }

    #[test]
    fn test_format_relative_hours() {
        let now = Timestamp::now();
        let dt = now - jiff::SignedDuration::from_secs(7200);
        let result = format_relative(dt, now);
        assert_eq!(result, "  2h ago ");
    }

    #[test]
    fn test_format_relative_days() {
        let now = Timestamp::now();
        let dt = now - jiff::SignedDuration::from_secs(172800);
        let result = format_relative(dt, now);
        assert_eq!(result, "  2d ago ");
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
        assert_eq!(spans[0].style.fg, Some(DARK.log_timestamp));
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
        assert_eq!(spans[0].style.fg, Some(DARK.log_timestamp));
    }

    #[test]
    fn test_colorize_utc_mode_keeps_original() {
        let spans = colorize_log_line("2026-02-24T16:36:51Z ERROR fail", &DARK, TimestampMode::Utc);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "2026-02-24T16:36:51Z ");
    }
}
