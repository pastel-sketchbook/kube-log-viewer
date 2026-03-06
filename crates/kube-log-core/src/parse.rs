//! Log line parsing utilities shared between the TUI and CLI interfaces.
//!
//! These functions handle timestamp detection/parsing, JSON log flattening,
//! and duplicate timestamp stripping.  They operate on plain `&str` slices
//! and have **no** dependency on `ratatui` or any other UI crate.

use std::borrow::Cow;
use std::sync::LazyLock;

use jiff::{Timestamp, civil, tz::TimeZone};
use regex::Regex;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Timestamp regex
// ---------------------------------------------------------------------------

/// Matches ISO 8601 / RFC 3339 timestamps at the start of a log line.
/// Covers K8s native format (`2024-01-15T10:00:00Z`, `…T10:00:00.123456789Z`)
/// and common application formats (`2024-01-15 10:00:00`, `…10:00:00,123`).
pub static TIMESTAMP_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Safety: this is a hardcoded literal that is guaranteed to compile.
    Regex::new(r"^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}([.,]\d+)?(Z|[+-]\d{2}:?\d{2})?\s*")
        .expect("hardcoded timestamp regex is valid")
});

// ---------------------------------------------------------------------------
// JSON flattening
// ---------------------------------------------------------------------------

/// Well-known field names for timestamp extraction.
pub const TIME_KEYS: &[&str] = &["time", "timestamp", "ts", "@timestamp", "datetime"];

/// Well-known field names for log-level extraction.
pub const LEVEL_KEYS: &[&str] = &["level", "severity", "loglevel", "log_level", "lvl"];

/// Well-known field names for message extraction.
pub const MSG_KEYS: &[&str] = &["msg", "message"];

/// Flatten a JSON object line into `timestamp [LEVEL] message key=value ...`.
///
/// Returns `Cow::Borrowed` for non-JSON lines (no allocation).
/// The output is designed to feed into `colorize_log_line()` so that
/// the extracted timestamp gets muted color and the level gets keyword color.
pub fn format_json_line(line: &str) -> Cow<'_, str> {
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

// ---------------------------------------------------------------------------
// Timestamp parsing
// ---------------------------------------------------------------------------

/// Try to parse a timestamp string into a [`jiff::Timestamp`].
///
/// Handles a wide range of ISO 8601 variants commonly found in K8s pod logs:
/// - RFC 3339 with timezone: `2026-02-24T16:36:51Z`, `...+05:30`
/// - RFC 3339 with fractional: `2026-02-24T16:36:51.600Z`, `.600000000Z`
/// - Comma fractional (Python logging): `2026-02-24 15:05:18,976`
/// - Dot fractional without TZ: `2026-02-24 15:05:18.976`, `...T15:05:18.976`
/// - Plain without fractional: `2026-02-24 15:05:18`, `...T15:05:18`
///
/// Timezone-less timestamps are assumed UTC.
pub fn parse_log_timestamp(ts: &str) -> Option<Timestamp> {
    let trimmed = ts.trim();

    // Normalise comma to dot so Python-style `18,976` becomes `18.976`.
    let normalised = trimmed.replace(',', ".");

    // 1. RFC 3339 (requires timezone designator)
    if let Ok(t) = normalised.parse::<Timestamp>() {
        return Some(t);
    }

    // 2. ISO variants without timezone — try most specific first.
    const NAIVE_FMTS: &[&str] = &[
        "%Y-%m-%dT%H:%M:%S%.f", // 2026-02-24T15:05:18.976
        "%Y-%m-%d %H:%M:%S%.f", // 2026-02-24 15:05:18.976
        "%Y-%m-%dT%H:%M:%S",    // 2026-02-24T15:05:18
        "%Y-%m-%d %H:%M:%S",    // 2026-02-24 15:05:18
    ];

    for fmt in NAIVE_FMTS {
        if let Ok(dt) = civil::DateTime::strptime(fmt, &normalised) {
            return dt.to_zoned(TimeZone::UTC).ok().map(|z| z.timestamp());
        }
    }

    None
}

/// Strip K8s-prepended timestamp when the remainder has its own timestamp.
/// This prevents showing two timestamps (K8s + application) on the same line.
/// Lines are kept as-is when the remainder does not start with a recognisable
/// timestamp (e.g. plain text or JSON with `{`).
pub fn strip_duplicate_timestamp(line: &str) -> &str {
    if let Some(m) = TIMESTAMP_RE.find(line) {
        let rest = &line[m.end()..];
        if TIMESTAMP_RE.is_match(rest) {
            return rest;
        }
    }
    line
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::tz::TimeZone;

    // -- parse_log_timestamp ------------------------------------------------

    #[test]
    fn test_parse_rfc3339_z() {
        let dt = parse_log_timestamp("2026-02-24T16:36:51Z").unwrap();
        assert_eq!(dt.as_second(), 1771951011);
    }

    #[test]
    fn test_parse_rfc3339_fractional() {
        let dt = parse_log_timestamp("2026-02-24T16:36:51.600Z").unwrap();
        assert_eq!(dt.as_second(), 1771951011);
    }

    #[test]
    fn test_parse_rfc3339_offset() {
        let dt = parse_log_timestamp("2026-02-24T16:36:51+05:30").unwrap();
        // 16:36:51 +05:30 = 11:06:51 UTC
        assert_eq!(dt.to_zoned(TimeZone::UTC).hour(), 11);
    }

    #[test]
    fn test_parse_space_separated() {
        let dt = parse_log_timestamp("2026-02-24 16:36:51").unwrap();
        assert_eq!(dt.as_second(), 1771951011);
    }

    #[test]
    fn test_parse_comma_fractional_space() {
        // Python logging format: `2026-02-24 15:05:18,976`
        let dt = parse_log_timestamp("2026-02-24 15:05:18,976").unwrap();
        assert_eq!(dt.as_second(), 1771945518);
    }

    #[test]
    fn test_parse_dot_fractional_space() {
        let dt = parse_log_timestamp("2026-02-24 15:05:18.976").unwrap();
        assert_eq!(dt.as_second(), 1771945518);
    }

    #[test]
    fn test_parse_dot_fractional_t_no_tz() {
        // ISO with T separator, fractional, but no timezone
        let dt = parse_log_timestamp("2026-02-24T15:05:18.976").unwrap();
        assert_eq!(dt.as_second(), 1771945518);
    }

    #[test]
    fn test_parse_comma_fractional_t_no_tz() {
        let dt = parse_log_timestamp("2026-02-24T15:05:18,976").unwrap();
        assert_eq!(dt.as_second(), 1771945518);
    }

    #[test]
    fn test_parse_t_no_fractional_no_tz() {
        let dt = parse_log_timestamp("2026-02-24T15:05:18").unwrap();
        assert_eq!(dt.as_second(), 1771945518);
    }

    #[test]
    fn test_parse_k8s_nanosecond() {
        // K8s native format with 9-digit fractional
        let dt = parse_log_timestamp("2026-02-24T16:36:51.600000000Z").unwrap();
        assert_eq!(dt.as_second(), 1771951011);
    }

    #[test]
    fn test_parse_comma_fractional_rfc3339() {
        // Comma fractional with timezone (rare but possible)
        let dt = parse_log_timestamp("2026-02-24T15:05:18,976Z").unwrap();
        assert_eq!(dt.as_second(), 1771945518);
    }

    #[test]
    fn test_parse_invalid_returns_none() {
        assert!(parse_log_timestamp("not-a-timestamp").is_none());
        assert!(parse_log_timestamp("").is_none());
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
    fn test_json_timestamp_feeds_regex() {
        // Verify the flattened output starts with a timestamp that TIMESTAMP_RE can match
        let line = r#"{"time":"2026-02-24T16:36:51.600Z","status":200}"#;
        let result = format_json_line(line);
        assert!(TIMESTAMP_RE.is_match(result.as_ref()));
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

    // -- strip_duplicate_timestamp -------------------------------------------

    #[test]
    fn test_strip_dup_ts_removes_k8s_prefix_when_app_has_own_timestamp() {
        // K8s prefix + application timestamp -> strip K8s prefix
        let line = "2026-02-24T16:36:51.600000000Z 2026-02-24T16:36:51.600Z [ERROR] fail";
        let result = strip_duplicate_timestamp(line);
        assert_eq!(result, "2026-02-24T16:36:51.600Z [ERROR] fail");
    }

    #[test]
    fn test_strip_dup_ts_keeps_line_when_no_app_timestamp() {
        // K8s prefix + plain text (no second timestamp) -> keep as-is
        let line = "2026-02-24T16:36:51.600000000Z plain text log message";
        let result = strip_duplicate_timestamp(line);
        assert_eq!(result, line);
    }

    #[test]
    fn test_strip_dup_ts_keeps_line_when_json_remainder() {
        // K8s prefix + JSON object -> keep as-is (JSON flattening handles it)
        let line = r#"2026-02-24T16:36:51.600Z {"time":"2026-02-24T16:36:51.600Z","msg":"hello"}"#;
        let result = strip_duplicate_timestamp(line);
        assert_eq!(result, line);
    }

    #[test]
    fn test_strip_dup_ts_keeps_line_when_no_timestamp_at_all() {
        // No timestamp at start -> keep as-is
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
        // Only one timestamp, no duplicate -> keep as-is
        let line = "2026-02-24T16:36:51Z ERROR something broke";
        let result = strip_duplicate_timestamp(line);
        assert_eq!(result, line);
    }
}
