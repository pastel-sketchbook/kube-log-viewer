//! Log line classifier for the anomaly detection pipeline.
//!
//! Every log line passes through [`Classifier::classify`] which assigns a
//! [`LineClass`] and produces a [`ClassifiedLine`]. Classification rules are
//! applied in priority order (see ADR-0007 §2):
//!
//! 1. **Error** — `ERROR`, `FATAL`, `PANIC`, HTTP 5xx
//! 2. **Warning** — `WARN`, `TIMEOUT`, `retry`, HTTP 4xx
//! 3. **Lifecycle** — `started`, `ready`, `shutdown`, `SIGTERM`, `pulling image`
//! 4. **HealthCheck** — `kube-probe`, `/healthz`, `/readyz`, `/livez`
//! 5. **Repeated** — structural dedup (normalize timestamps/UUIDs/IPs/numbers)
//! 6. **Novel** — first-seen canonical form
//! 7. **Normal** — everything else

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::parse::{self, TIMESTAMP_RE};
use crate::types::{ClassifiedLine, LineClass};

// ---------------------------------------------------------------------------
// Normalization regexes for structural dedup
// ---------------------------------------------------------------------------

/// UUID pattern: 8-4-4-4-12 hex digits.
static UUID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
        .expect("hardcoded UUID regex is valid")
});

/// IPv4 address.
static IPV4_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b").expect("hardcoded IPv4 regex is valid")
});

/// Hex strings (8+ chars, e.g. request IDs, hashes).
static HEX_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[0-9a-fA-F]{8,}\b").expect("hardcoded hex ID regex is valid"));

/// Standalone numeric values (integers and decimals). Uses a left word
/// boundary but allows the number to be followed by letter suffixes like
/// `ms`, `Gi`, `s`, etc. (common in log lines for durations/sizes).
static NUMERIC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d+(\.\d+)?").expect("hardcoded numeric regex is valid"));

// ---------------------------------------------------------------------------
// Classification keyword patterns
// ---------------------------------------------------------------------------

/// Error-level indicators (case-insensitive matching done via uppercase conversion).
const ERROR_KEYWORDS: &[&str] = &["ERROR", "FATAL", "PANIC", "panic:", "CRIT", "CRITICAL"];

/// Error patterns that need substring matching (not word-boundary).
const ERROR_PATTERNS: &[&str] = &[
    "OOMKilled",
    "OutOfMemory",
    "connection refused",
    "segfault",
    "stack trace",
    "stacktrace",
    "traceback",
    "Traceback",
];

/// Warning-level indicators.
const WARN_KEYWORDS: &[&str] = &["WARN", "WARNING"];

/// Warning patterns.
const WARN_PATTERNS: &[&str] = &[
    "timeout",
    "TIMEOUT",
    "retry",
    "retrying",
    "deprecated",
    "deprecation",
];

/// Lifecycle / state transition indicators.
const LIFECYCLE_PATTERNS: &[&str] = &[
    "started",
    "Starting",
    "ready",
    "Ready",
    "shutdown",
    "shutting down",
    "Shutting down",
    "SIGTERM",
    "SIGKILL",
    "pulling image",
    "Pulling image",
    "container created",
    "container killed",
    "Liveness probe",
    "Readiness probe",
    "CrashLoopBackOff",
    "BackOff",
    "restarting",
    "Restarting",
    "Stopping",
    "Terminated",
    "OOMKilled",
];

/// Health check indicators.
const HEALTHCHECK_PATTERNS: &[&str] = &[
    "kube-probe",
    "/healthz",
    "/readyz",
    "/livez",
    "/health",
    "health check",
    "healthcheck",
    "liveness",
    "readiness",
    "startup probe",
];

/// HTTP 5xx status code pattern.
static HTTP_5XX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b5\d{2}\b").expect("hardcoded HTTP 5xx regex is valid"));

/// HTTP 4xx status code pattern.
static HTTP_4XX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b4\d{2}\b").expect("hardcoded HTTP 4xx regex is valid"));

// ---------------------------------------------------------------------------
// Classifier
// ---------------------------------------------------------------------------

/// Maximum entries in the seen-set before LRU eviction kicks in.
const MAX_SEEN_SET_SIZE: usize = 10_000;

/// Stateful log line classifier with structural dedup tracking.
///
/// Maintains a seen-set of canonical forms (normalized log patterns) to detect
/// repeated and novel lines. The seen-set is bounded by [`MAX_SEEN_SET_SIZE`]
/// with simple eviction (clear when full — simpler than true LRU, acceptable
/// for streaming logs where recent patterns matter most).
pub struct Classifier {
    /// Map from canonical form → (count, first raw example).
    seen: HashMap<String, (u32, String)>,
}

impl Classifier {
    /// Create a new classifier with an empty seen-set.
    pub fn new() -> Self {
        Self {
            seen: HashMap::new(),
        }
    }

    /// Classify a single raw log line.
    ///
    /// `pod` and `container` identify the source. The classifier extracts
    /// timestamp, level, message, and structured fields (for JSON lines),
    /// then assigns a [`LineClass`] based on content analysis and dedup state.
    pub fn classify(&mut self, raw: &str, pod: &str, container: Option<&str>) -> ClassifiedLine {
        // Parse timestamp from the raw line.
        let timestamp = TIMESTAMP_RE
            .find(raw)
            .and_then(|m| parse::parse_log_timestamp(m.as_str().trim()));

        // Try to extract structured fields from JSON.
        let (level, msg, fields) = extract_json_fields(raw);

        // The text to classify: prefer the message field, fall back to raw.
        let classify_text = msg.as_deref().unwrap_or(raw);

        // Determine the level string: from JSON extraction or from raw text.
        let level = level.or_else(|| detect_level_from_text(raw));

        // Classify in priority order.
        let class = self.classify_text(classify_text, raw);

        ClassifiedLine {
            timestamp,
            pod: pod.to_string(),
            container: container.map(|s| s.to_string()),
            class,
            level,
            msg,
            raw: raw.to_string(),
            fields,
        }
    }

    /// Core classification logic applied to the message text.
    fn classify_text(&mut self, text: &str, raw: &str) -> LineClass {
        let upper = text.to_uppercase();

        // 1. Error detection
        if is_error(&upper, text) {
            return LineClass::Error;
        }

        // 2. Warning detection
        if is_warning(&upper, text) {
            return LineClass::Warning;
        }

        // 3. Health check detection (before lifecycle, because health check
        //    URLs like `/readyz` contain lifecycle words like "ready")
        if is_healthcheck(text) {
            return LineClass::HealthCheck;
        }

        // 4. Lifecycle detection
        if is_lifecycle(text) {
            return LineClass::Lifecycle;
        }

        // 5. Structural dedup: normalize and check seen-set.
        let canonical = normalize(raw);
        self.classify_by_novelty(canonical, raw)
    }

    /// Check the seen-set for novelty/repetition.
    fn classify_by_novelty(&mut self, canonical: String, raw: &str) -> LineClass {
        // Evict if at capacity (simple strategy: clear the map).
        if self.seen.len() >= MAX_SEEN_SET_SIZE {
            self.seen.clear();
        }

        if let Some((count, _sample)) = self.seen.get_mut(&canonical) {
            *count += 1;
            LineClass::Repeated {
                count: *count,
                canonical,
            }
        } else {
            self.seen.insert(canonical, (1, raw.to_string()));
            LineClass::Novel
        }
    }

    /// Reset the seen-set (e.g. when switching pods/streams).
    pub fn reset(&mut self) {
        self.seen.clear();
    }

    /// Current number of canonical forms tracked.
    pub fn seen_count(&self) -> usize {
        self.seen.len()
    }
}

impl Default for Classifier {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Classification helpers
// ---------------------------------------------------------------------------

/// Check if the line indicates an error.
fn is_error(upper: &str, original: &str) -> bool {
    // Keyword match (uppercased)
    for kw in ERROR_KEYWORDS {
        if upper.contains(kw) {
            return true;
        }
    }
    // Pattern match (case-sensitive where needed)
    for pat in ERROR_PATTERNS {
        if original.contains(pat) {
            return true;
        }
    }
    // HTTP 5xx (but not in timestamps or other numeric contexts — check
    // that the match isn't part of a larger number context by requiring
    // it to appear with "status" or "HTTP" nearby, or as a standalone).
    if HTTP_5XX_RE.is_match(original) && has_http_context(original) {
        return true;
    }
    false
}

/// Check if the line indicates a warning.
fn is_warning(upper: &str, original: &str) -> bool {
    for kw in WARN_KEYWORDS {
        if upper.contains(kw) {
            return true;
        }
    }
    for pat in WARN_PATTERNS {
        if original.contains(pat) {
            return true;
        }
    }
    if HTTP_4XX_RE.is_match(original) && has_http_context(original) {
        return true;
    }
    false
}

/// Check if the 3-digit status code appears in an HTTP-like context.
fn has_http_context(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("status")
        || lower.contains("http")
        || lower.contains("response")
        || lower.contains("status_code")
        || lower.contains("status=")
        || lower.contains("code=")
}

/// Check if the line indicates a lifecycle/state transition.
fn is_lifecycle(text: &str) -> bool {
    for pat in LIFECYCLE_PATTERNS {
        if text.contains(pat) {
            return true;
        }
    }
    false
}

/// Check if the line indicates a health check.
fn is_healthcheck(text: &str) -> bool {
    let lower = text.to_lowercase();
    for pat in HEALTHCHECK_PATTERNS {
        if lower.contains(pat) {
            return true;
        }
    }
    false
}

/// Try to detect log level from the raw text (non-JSON lines).
fn detect_level_from_text(text: &str) -> Option<String> {
    // Look for common level patterns: [ERROR], [WARN], ERROR, WARN, INFO, etc.
    let upper = text.to_uppercase();
    if upper.contains("ERROR") || upper.contains("FATAL") || upper.contains("PANIC") {
        Some("ERROR".to_string())
    } else if upper.contains("WARN") {
        Some("WARN".to_string())
    } else if upper.contains("INFO") {
        Some("INFO".to_string())
    } else if upper.contains("DEBUG") {
        Some("DEBUG".to_string())
    } else if upper.contains("TRACE") {
        Some("TRACE".to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// JSON field extraction
// ---------------------------------------------------------------------------

/// Extract level, message, and remaining fields from a JSON log line.
///
/// Returns `(level, msg, fields)`. For non-JSON lines, returns `(None, None, None)`.
fn extract_json_fields(
    raw: &str,
) -> (
    Option<String>,
    Option<String>,
    Option<serde_json::Map<String, Value>>,
) {
    // Skip optional K8s timestamp prefix.
    let json_part = match TIMESTAMP_RE.find(raw) {
        Some(m) if raw[m.end()..].starts_with('{') => &raw[m.end()..],
        _ if raw.starts_with('{') => raw,
        _ => return (None, None, None),
    };

    let obj = match serde_json::from_str::<Value>(json_part) {
        Ok(Value::Object(map)) => map,
        _ => return (None, None, None),
    };

    let mut level = None;
    let mut msg = None;
    let mut remaining = serde_json::Map::new();

    for (key, val) in &obj {
        let key_lower = key.to_lowercase();
        if parse::LEVEL_KEYS.contains(&key_lower.as_str()) {
            if let Some(s) = val.as_str() {
                level = Some(s.to_uppercase());
            }
        } else if parse::MSG_KEYS.contains(&key_lower.as_str()) {
            if let Some(s) = val.as_str() {
                msg = Some(s.to_string());
            }
        } else if !parse::TIME_KEYS.contains(&key_lower.as_str()) {
            remaining.insert(key.clone(), val.clone());
        }
    }

    let fields = if remaining.is_empty() {
        None
    } else {
        Some(remaining)
    };

    (level, msg, fields)
}

// ---------------------------------------------------------------------------
// Structural normalization for dedup
// ---------------------------------------------------------------------------

/// Normalize a log line for structural dedup.
///
/// Strips timestamps, replaces UUIDs with `<uuid>`, IPv4 with `<ip>`,
/// hex IDs with `<hex>`, and numeric values with `<num>`.
/// The result is a canonical form where structurally identical lines
/// (differing only in dynamic values) produce the same string.
pub fn normalize(line: &str) -> String {
    // 1. Strip leading timestamp(s).
    let rest = match TIMESTAMP_RE.find(line) {
        Some(m) => {
            let after = &line[m.end()..];
            // Also strip a second timestamp if present (K8s prefix + app timestamp).
            match TIMESTAMP_RE.find(after) {
                Some(m2) => &after[m2.end()..],
                None => after,
            }
        }
        None => line,
    };

    // 2. Replace UUIDs.
    let result = UUID_RE.replace_all(rest, "<uuid>");
    // 3. Replace IPv4 addresses.
    let result = IPV4_RE.replace_all(&result, "<ip>");
    // 4. Replace hex IDs (8+ hex chars).
    let result = HEX_ID_RE.replace_all(&result, "<hex>");
    // 5. Replace standalone numbers.
    let result = NUMERIC_RE.replace_all(&result, "<num>");

    result.to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- normalize ----------------------------------------------------------

    #[test]
    fn test_normalize_strips_timestamp() {
        let line = "2026-03-06T10:15:23Z INFO GET /api/users";
        let norm = normalize(line);
        assert!(!norm.contains("2026"));
        assert!(norm.contains("INFO"));
    }

    #[test]
    fn test_normalize_strips_double_timestamp() {
        let line = "2026-03-06T10:15:23.000000000Z 2026-03-06T10:15:23Z INFO started";
        let norm = normalize(line);
        assert!(!norm.contains("2026"));
        assert!(norm.contains("INFO"));
    }

    #[test]
    fn test_normalize_replaces_uuid() {
        let line = "request_id=a1b2c3d4-e5f6-7890-abcd-ef1234567890 done";
        let norm = normalize(line);
        assert!(norm.contains("<uuid>"));
        assert!(!norm.contains("a1b2c3d4"));
    }

    #[test]
    fn test_normalize_replaces_ipv4() {
        let line = "connection from 192.168.1.100 accepted";
        let norm = normalize(line);
        assert!(norm.contains("<ip>"));
        assert!(!norm.contains("192.168.1.100"));
    }

    #[test]
    fn test_normalize_replaces_numbers() {
        let line = "latency=42ms bytes=1024";
        let norm = normalize(line);
        assert!(norm.contains("<num>"));
        assert!(!norm.contains("42"));
        assert!(!norm.contains("1024"));
    }

    #[test]
    fn test_normalize_replaces_hex_ids() {
        let line = "trace_id=abcdef0123456789 span=fedcba98";
        let norm = normalize(line);
        assert!(norm.contains("<hex>"));
        assert!(!norm.contains("abcdef0123456789"));
    }

    #[test]
    fn test_normalize_structural_dedup_example_from_adr() {
        let line1 = "2026-03-06T10:15:23Z INFO  GET /api/users request_id=a1b2c3d4-e5f6-7890-abcd-ef1234567890 latency=12ms";
        let line2 = "2026-03-06T10:15:24Z INFO  GET /api/users request_id=d4e5f6a7-b8c9-0123-4567-890abcdef012 latency=14ms";
        assert_eq!(normalize(line1), normalize(line2));
    }

    // -- classify -----------------------------------------------------------

    #[test]
    fn test_classify_error_keyword() {
        let mut c = Classifier::new();
        let result = c.classify(
            "2026-03-06T10:15:23Z ERROR connection refused to db:5432",
            "pod-1",
            Some("app"),
        );
        assert_eq!(result.class, LineClass::Error);
        assert_eq!(result.pod, "pod-1");
        assert_eq!(result.container.as_deref(), Some("app"));
        assert!(result.timestamp.is_some());
    }

    #[test]
    fn test_classify_fatal() {
        let mut c = Classifier::new();
        let result = c.classify("FATAL: unable to allocate memory", "pod-1", None);
        assert_eq!(result.class, LineClass::Error);
    }

    #[test]
    fn test_classify_panic() {
        let mut c = Classifier::new();
        let result = c.classify("panic: runtime error: index out of range", "pod-1", None);
        assert_eq!(result.class, LineClass::Error);
    }

    #[test]
    fn test_classify_http_5xx() {
        let mut c = Classifier::new();
        let result = c.classify("GET /api/users status=503 latency=5ms", "pod-1", None);
        assert_eq!(result.class, LineClass::Error);
    }

    #[test]
    fn test_classify_warning_keyword() {
        let mut c = Classifier::new();
        let result = c.classify("2026-03-06T10:15:23Z WARN disk usage at 85%", "pod-1", None);
        assert_eq!(result.class, LineClass::Warning);
    }

    #[test]
    fn test_classify_timeout_warning() {
        let mut c = Classifier::new();
        let result = c.classify("request timeout after 30s", "pod-1", None);
        assert_eq!(result.class, LineClass::Warning);
    }

    #[test]
    fn test_classify_http_4xx() {
        let mut c = Classifier::new();
        let result = c.classify("GET /api/users status=404 latency=2ms", "pod-1", None);
        assert_eq!(result.class, LineClass::Warning);
    }

    #[test]
    fn test_classify_lifecycle_started() {
        let mut c = Classifier::new();
        let result = c.classify("server started on port 8080", "pod-1", None);
        assert_eq!(result.class, LineClass::Lifecycle);
    }

    #[test]
    fn test_classify_lifecycle_sigterm() {
        let mut c = Classifier::new();
        let result = c.classify("received SIGTERM, shutting down gracefully", "pod-1", None);
        assert_eq!(result.class, LineClass::Lifecycle);
    }

    #[test]
    fn test_classify_healthcheck_kube_probe() {
        let mut c = Classifier::new();
        let result = c.classify("GET /healthz kube-probe/1.28 200 OK", "pod-1", None);
        assert_eq!(result.class, LineClass::HealthCheck);
    }

    #[test]
    fn test_classify_healthcheck_readyz() {
        let mut c = Classifier::new();
        let result = c.classify("GET /readyz 200 OK", "pod-1", None);
        assert_eq!(result.class, LineClass::HealthCheck);
    }

    #[test]
    fn test_classify_novel_then_repeated() {
        let mut c = Classifier::new();

        // First occurrence → Novel
        // All three lines share identical structure after normalization:
        //   "INFO GET /api/users request_id=req-<num> latency=<num>ms"
        // Only the numeric portions differ.
        let r1 = c.classify(
            "INFO GET /api/users request_id=req-111 latency=12ms",
            "pod-1",
            None,
        );
        assert_eq!(r1.class, LineClass::Novel);

        // Second occurrence (same structure, different values): repeated
        let r2 = c.classify(
            "INFO GET /api/users request_id=req-222 latency=14ms",
            "pod-1",
            None,
        );
        match &r2.class {
            LineClass::Repeated { count, .. } => assert_eq!(*count, 2),
            other => panic!("expected Repeated, got {:?}", other),
        }

        // Third occurrence: count increases
        let r3 = c.classify(
            "INFO GET /api/users request_id=req-333 latency=9ms",
            "pod-1",
            None,
        );
        match &r3.class {
            LineClass::Repeated { count, .. } => assert_eq!(*count, 3),
            other => panic!("expected Repeated, got {:?}", other),
        }
    }

    #[test]
    fn test_classify_normal_line() {
        // A line that matches no error/warning/lifecycle/healthcheck patterns
        // AND has a unique structure → Novel first time. A completely different
        // normal line → also Novel. Only the _same_ structure repeated → Repeated.
        let mut c = Classifier::new();
        let r = c.classify("INFO processing batch job XYZ", "pod-1", None);
        // First occurrence of this pattern → Novel
        assert_eq!(r.class, LineClass::Novel);
    }

    #[test]
    fn test_classify_json_line() {
        let mut c = Classifier::new();
        let line = r#"{"level":"error","msg":"connection refused to db:5432","status":500}"#;
        let result = c.classify(line, "pod-1", Some("app"));
        assert_eq!(result.class, LineClass::Error);
        assert_eq!(result.level.as_deref(), Some("ERROR"));
        assert_eq!(result.msg.as_deref(), Some("connection refused to db:5432"));
        assert!(result.fields.is_some());
        assert!(result.fields.as_ref().unwrap().contains_key("status"));
    }

    #[test]
    fn test_classify_json_with_k8s_prefix() {
        let mut c = Classifier::new();
        let line =
            r#"2026-03-06T10:15:23.000Z {"level":"warn","msg":"retrying request","attempt":3}"#;
        let result = c.classify(line, "pod-1", None);
        assert_eq!(result.class, LineClass::Warning);
        assert_eq!(result.level.as_deref(), Some("WARN"));
        assert!(result.timestamp.is_some());
    }

    #[test]
    fn test_classifier_reset() {
        let mut c = Classifier::new();
        c.classify("INFO some line abc123de", "pod-1", None);
        assert_eq!(c.seen_count(), 1);
        c.reset();
        assert_eq!(c.seen_count(), 0);
    }

    #[test]
    fn test_seen_set_eviction() {
        let mut c = Classifier::new();
        // Fill the seen-set to capacity with structurally distinct lines.
        // Each line has a different word (not just a different number) so
        // normalization produces distinct canonical forms.
        for i in 0..MAX_SEEN_SET_SIZE {
            // Use a unique alphabetic suffix so normalization doesn't collapse them.
            let suffix = format_alpha(i);
            c.classify(&format!("GET /api/{suffix} completed"), "pod-1", None);
        }
        assert_eq!(c.seen_count(), MAX_SEEN_SET_SIZE);

        // Next classify should trigger eviction (clear).
        c.classify(
            "one more unique line triggering eviction zzzzz",
            "pod-1",
            None,
        );
        // After clear + inserting the new one, count should be 1.
        assert_eq!(c.seen_count(), 1);
    }

    /// Convert a number to a unique alphabetic string (a, b, ..., z, aa, ab, ...).
    fn format_alpha(mut n: usize) -> String {
        let mut s = String::new();
        loop {
            s.push((b'a' + (n % 26) as u8) as char);
            n /= 26;
            if n == 0 {
                break;
            }
            n -= 1; // Adjust for base-26 without zero
        }
        s
    }

    #[test]
    fn test_error_priority_over_lifecycle() {
        // "OOMKilled" appears in both error and lifecycle patterns.
        // Error should take priority.
        let mut c = Classifier::new();
        let result = c.classify("container OOMKilled after using 4Gi memory", "pod-1", None);
        assert_eq!(result.class, LineClass::Error);
    }

    #[test]
    fn test_healthcheck_case_insensitive() {
        let mut c = Classifier::new();
        let result = c.classify("GET /HEALTHZ 200 OK", "pod-1", None);
        assert_eq!(result.class, LineClass::HealthCheck);
    }

    #[test]
    fn test_5xx_without_http_context_is_not_error() {
        // A line with "500" but no HTTP context should NOT be classified as error.
        let mut c = Classifier::new();
        let result = c.classify("processed 500 records successfully", "pod-1", None);
        assert_ne!(result.class, LineClass::Error);
    }

    #[test]
    fn test_extract_json_fields_non_json() {
        let (level, msg, fields) = extract_json_fields("plain text line");
        assert!(level.is_none());
        assert!(msg.is_none());
        assert!(fields.is_none());
    }

    #[test]
    fn test_extract_json_fields_with_level_and_msg() {
        let line = r#"{"level":"info","msg":"server started","port":8080}"#;
        let (level, msg, fields) = extract_json_fields(line);
        assert_eq!(level.as_deref(), Some("INFO"));
        assert_eq!(msg.as_deref(), Some("server started"));
        assert!(fields.is_some());
        assert!(fields.unwrap().contains_key("port"));
    }

    #[test]
    fn test_extract_json_fields_strips_time_keys() {
        let line = r#"{"time":"2026-03-06T10:00:00Z","level":"error","msg":"fail"}"#;
        let (_level, _msg, fields) = extract_json_fields(line);
        // "time" should be consumed, not in remaining fields
        assert!(fields.is_none()); // Only time, level, msg — nothing remaining
    }

    #[test]
    fn test_detect_level_from_text() {
        assert_eq!(
            detect_level_from_text("some ERROR happened"),
            Some("ERROR".to_string())
        );
        assert_eq!(
            detect_level_from_text("WARN: low disk"),
            Some("WARN".to_string())
        );
        assert_eq!(
            detect_level_from_text("INFO starting up"),
            Some("INFO".to_string())
        );
        assert_eq!(
            detect_level_from_text("DEBUG verbose output"),
            Some("DEBUG".to_string())
        );
        assert_eq!(detect_level_from_text("no level here"), None);
    }
}
