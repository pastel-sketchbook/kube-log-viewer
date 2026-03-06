//! Shared types for the classify-filter-reduce pipeline.
//!
//! These types flow through the anomaly detection pipeline:
//!
//! 1. Raw log line → [`ClassifiedLine`] (via `classify.rs`)
//! 2. [`ClassifiedLine`] → kept/dropped (via `filter.rs`)
//! 3. Kept lines → [`Summary`] + filtered [`ClassifiedLine`]s (via `reduce.rs`)
//! 4. [`Summary`] + lines → JSON/JSONL/plain text (via `export.rs`)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Map as JsonMap;
use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// The semantic class assigned to a log line by the classifier.
///
/// Applied in priority order: Error > Warning > Lifecycle > HealthCheck >
/// Repeated > Novel > Normal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LineClass {
    /// Error-level: panics, stack traces, HTTP 5xx, connection refused, OOM.
    Error,
    /// Warning-level: retries, timeouts, deprecation notices, HTTP 4xx.
    Warning,
    /// State transition: pod started, container ready, graceful shutdown,
    /// config reload.
    Lifecycle,
    /// First occurrence of a message pattern not seen before in this stream.
    Novel,
    /// Health check: kube-probe, liveness/readiness/startup probe responses.
    HealthCheck,
    /// Structurally identical to a line already seen (modulo timestamp,
    /// request-id, IP, UUID, numeric values).
    Repeated {
        /// How many times this canonical form has been seen so far.
        count: u32,
        /// The normalized form used for deduplication.
        canonical: String,
    },
    /// Normal: info-level, routine operational log.
    Normal,
}

impl LineClass {
    /// Short label used in JSON output as the `class` field value.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Lifecycle => "lifecycle",
            Self::Novel => "novel",
            Self::HealthCheck => "healthcheck",
            Self::Repeated { .. } => "repeated",
            Self::Normal => "normal",
        }
    }
}

// ---------------------------------------------------------------------------
// Classified line
// ---------------------------------------------------------------------------

/// A log line after classification. Carries the original text, parsed
/// metadata, and the assigned [`LineClass`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifiedLine {
    /// ISO 8601 UTC timestamp, when parseable from the log line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,

    /// Source pod name.
    pub pod: String,

    /// Container name within the pod, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,

    /// Assigned classification.
    pub class: LineClass,

    /// Original log level string as found in the line (e.g. "ERROR", "WARN").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,

    /// Human-readable message extracted from the log line.
    /// For JSON logs, this is the `msg`/`message` field value.
    /// For plain text, this is the non-timestamp portion of the line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,

    /// Verbatim log line from the K8s API.
    pub raw: String,

    /// Remaining structured fields when the log line was JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<JsonMap<String, JsonValue>>,
}

// ---------------------------------------------------------------------------
// Collapsed line (for suppressed repeated groups)
// ---------------------------------------------------------------------------

/// A placeholder for a group of suppressed repeated lines.
/// Emitted in JSONL output instead of the individual repeated lines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollapsedGroup {
    /// Always `true` — marker for consumers to distinguish from regular lines.
    #[serde(rename = "_collapsed")]
    pub collapsed: bool,

    /// Number of suppressed repetitions.
    pub count: u32,

    /// Normalized pattern that was repeated.
    pub canonical: String,

    /// One verbatim example of the repeated line.
    pub sample: String,
}

// ---------------------------------------------------------------------------
// Summary types (reduce output)
// ---------------------------------------------------------------------------

/// Top-level summary produced by the reduce pipeline.
/// Bounded in size regardless of input volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    /// Time range of the analyzed log lines `[first, last]`.
    pub time_range: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),

    /// Total number of log lines received (before filtering).
    pub total_lines: u64,

    /// Number of lines suppressed by the troubleshoot filter.
    pub suppressed_lines: u64,

    /// Count of error-class lines.
    pub error_count: u64,

    /// Count of warning-class lines.
    pub warning_count: u64,

    /// Per-pod summaries.
    pub pods: Vec<PodSummary>,

    /// Deduplicated error patterns, ranked by frequency.
    /// Capped at 20 buckets.
    pub top_errors: Vec<ErrorBucket>,

    /// Deduplicated warning patterns, ranked by frequency.
    /// Capped at 20 buckets.
    pub top_warnings: Vec<ErrorBucket>,

    /// Error/warning rate per time bucket (1 minute granularity).
    /// Capped at 60 entries (last hour).
    pub timeline: Vec<TimelineEntry>,

    /// First-seen patterns that may indicate root cause.
    /// Capped at 30 entries.
    pub novel_patterns: Vec<String>,

    /// Pod restart events observed during the time range.
    pub restart_events: Vec<RestartEvent>,
}

/// Summary info for a single pod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodSummary {
    /// Pod name.
    pub name: String,

    /// Current pod status (e.g. "Running", "CrashLoopBackOff").
    pub status: String,

    /// Total restart count across all containers.
    pub restarts: i32,
}

/// A deduplicated error/warning bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBucket {
    /// Normalized error/warning pattern (timestamps, UUIDs, IPs removed).
    pub canonical: String,

    /// How many times this pattern appeared.
    pub count: u64,

    /// When this pattern was first seen.
    pub first_seen: DateTime<Utc>,

    /// When this pattern was last seen.
    pub last_seen: DateTime<Utc>,

    /// One verbatim example for the LLM to read.
    pub sample: String,
}

/// A single time bucket in the error/warning timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// Start of the time bucket (1-minute granularity).
    pub bucket_start: DateTime<Utc>,

    /// Number of error-class lines in this bucket.
    pub error_count: u64,

    /// Number of warning-class lines in this bucket.
    pub warning_count: u64,

    /// Number of novel-pattern lines in this bucket.
    pub novel_count: u64,
}

/// A pod restart event observed during log analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartEvent {
    /// Pod that restarted.
    pub pod: String,

    /// When the restart was observed.
    pub at: DateTime<Utc>,

    /// Reason string from K8s (e.g. "CrashLoopBackOff", "OOMKilled").
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Filter configuration
// ---------------------------------------------------------------------------

/// Which line classes to include in filtered output.
///
/// Used by the filter stage to determine which classified lines pass through.
#[derive(Debug, Clone)]
pub struct FilterConfig {
    /// Include error-class lines.
    pub error: bool,
    /// Include warning-class lines.
    pub warning: bool,
    /// Include lifecycle-class lines.
    pub lifecycle: bool,
    /// Include novel-class lines.
    pub novel: bool,
    /// Include health-check-class lines.
    pub healthcheck: bool,
    /// Include normal-class lines.
    pub normal: bool,
    /// Include repeated-class lines (individually, not collapsed).
    pub repeated: bool,
}

impl FilterConfig {
    /// Default troubleshoot mode: keep errors, warnings, lifecycle, novel.
    /// Suppress health checks, repeated, and normal.
    pub fn troubleshoot() -> Self {
        Self {
            error: true,
            warning: true,
            lifecycle: true,
            novel: true,
            healthcheck: false,
            normal: false,
            repeated: false,
        }
    }

    /// Verbose mode: include normal lines but still suppress health checks.
    pub fn verbose() -> Self {
        Self {
            error: true,
            warning: true,
            lifecycle: true,
            novel: true,
            healthcheck: false,
            normal: true,
            repeated: false,
        }
    }

    /// All mode: show everything, no filtering.
    pub fn all() -> Self {
        Self {
            error: true,
            warning: true,
            lifecycle: true,
            novel: true,
            healthcheck: true,
            normal: true,
            repeated: true,
        }
    }

    /// Build from a comma-separated include list (e.g. "error,warning,lifecycle").
    pub fn from_include_list(classes: &str) -> Self {
        let mut config = Self {
            error: false,
            warning: false,
            lifecycle: false,
            novel: false,
            healthcheck: false,
            normal: false,
            repeated: false,
        };
        for class in classes.split(',') {
            match class.trim() {
                "error" => config.error = true,
                "warning" => config.warning = true,
                "lifecycle" => config.lifecycle = true,
                "novel" => config.novel = true,
                "healthcheck" => config.healthcheck = true,
                "normal" => config.normal = true,
                "repeated" => config.repeated = true,
                _ => {} // Unknown classes silently ignored
            }
        }
        config
    }

    /// Returns `true` if the given [`LineClass`] should be included.
    pub fn should_include(&self, class: &LineClass) -> bool {
        match class {
            LineClass::Error => self.error,
            LineClass::Warning => self.warning,
            LineClass::Lifecycle => self.lifecycle,
            LineClass::Novel => self.novel,
            LineClass::HealthCheck => self.healthcheck,
            LineClass::Repeated { .. } => self.repeated,
            LineClass::Normal => self.normal,
        }
    }
}

// ---------------------------------------------------------------------------
// Output format
// ---------------------------------------------------------------------------

/// Output format for the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Single JSON object with `summary` + `lines` array.
    Json,
    /// One JSON object per line (streaming-friendly).
    Jsonl,
    /// Plain text (human-readable fallback).
    Plain,
}

// ---------------------------------------------------------------------------
// Pipeline output
// ---------------------------------------------------------------------------

/// The complete output of the classify-filter-reduce pipeline.
/// This is the top-level structure serialized to JSON for CLI output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineOutput {
    /// Natural-language preamble for LLM consumption.
    /// Describes what data this is, what was filtered, and the scale of
    /// reduction.
    #[serde(rename = "_hint")]
    pub hint: String,

    /// Compressed situational overview.
    pub summary: Summary,

    /// The classified-and-filtered log lines (errors, warnings, lifecycle,
    /// novel patterns).
    pub lines: Vec<ClassifiedLine>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_class_labels() {
        assert_eq!(LineClass::Error.label(), "error");
        assert_eq!(LineClass::Warning.label(), "warning");
        assert_eq!(LineClass::Lifecycle.label(), "lifecycle");
        assert_eq!(LineClass::Novel.label(), "novel");
        assert_eq!(LineClass::HealthCheck.label(), "healthcheck");
        assert_eq!(
            LineClass::Repeated {
                count: 5,
                canonical: "test".into()
            }
            .label(),
            "repeated"
        );
        assert_eq!(LineClass::Normal.label(), "normal");
    }

    #[test]
    fn test_filter_config_troubleshoot() {
        let cfg = FilterConfig::troubleshoot();
        assert!(cfg.should_include(&LineClass::Error));
        assert!(cfg.should_include(&LineClass::Warning));
        assert!(cfg.should_include(&LineClass::Lifecycle));
        assert!(cfg.should_include(&LineClass::Novel));
        assert!(!cfg.should_include(&LineClass::HealthCheck));
        assert!(!cfg.should_include(&LineClass::Normal));
        assert!(!cfg.should_include(&LineClass::Repeated {
            count: 1,
            canonical: String::new()
        }));
    }

    #[test]
    fn test_filter_config_verbose() {
        let cfg = FilterConfig::verbose();
        assert!(cfg.should_include(&LineClass::Normal));
        assert!(!cfg.should_include(&LineClass::HealthCheck));
    }

    #[test]
    fn test_filter_config_all() {
        let cfg = FilterConfig::all();
        assert!(cfg.should_include(&LineClass::HealthCheck));
        assert!(cfg.should_include(&LineClass::Normal));
        assert!(cfg.should_include(&LineClass::Repeated {
            count: 1,
            canonical: String::new()
        }));
    }

    #[test]
    fn test_filter_config_from_include_list() {
        let cfg = FilterConfig::from_include_list("error,lifecycle,novel");
        assert!(cfg.should_include(&LineClass::Error));
        assert!(!cfg.should_include(&LineClass::Warning));
        assert!(cfg.should_include(&LineClass::Lifecycle));
        assert!(cfg.should_include(&LineClass::Novel));
        assert!(!cfg.should_include(&LineClass::HealthCheck));
    }

    #[test]
    fn test_filter_config_from_include_list_with_spaces() {
        let cfg = FilterConfig::from_include_list("error , warning , healthcheck");
        assert!(cfg.should_include(&LineClass::Error));
        assert!(cfg.should_include(&LineClass::Warning));
        assert!(cfg.should_include(&LineClass::HealthCheck));
        assert!(!cfg.should_include(&LineClass::Normal));
    }

    #[test]
    fn test_filter_config_from_include_list_unknown_ignored() {
        let cfg = FilterConfig::from_include_list("error,bogus,warning");
        assert!(cfg.should_include(&LineClass::Error));
        assert!(cfg.should_include(&LineClass::Warning));
        assert!(!cfg.should_include(&LineClass::Normal));
    }

    #[test]
    fn test_classified_line_json_round_trip() {
        let line = ClassifiedLine {
            timestamp: Some(Utc::now()),
            pod: "payments-7f8d".to_string(),
            container: Some("app".to_string()),
            class: LineClass::Error,
            level: Some("ERROR".to_string()),
            msg: Some("connection refused to db:5432".to_string()),
            raw: "2026-03-06T10:15:23Z ERROR connection refused to db:5432".to_string(),
            fields: None,
        };

        let json = serde_json::to_string(&line).expect("serialize");
        let back: ClassifiedLine = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.pod, "payments-7f8d");
        assert_eq!(back.class, LineClass::Error);
    }

    #[test]
    fn test_collapsed_group_json() {
        let group = CollapsedGroup {
            collapsed: true,
            count: 847,
            canonical: "INFO GET /api/users request_id=<id> latency=<num>ms".to_string(),
            sample: "2026-03-06T10:15:25Z INFO GET /api/users request_id=a1b2c3 latency=12ms"
                .to_string(),
        };

        let json = serde_json::to_string(&group).expect("serialize");
        assert!(json.contains("\"_collapsed\":true"));
        assert!(json.contains("\"count\":847"));
    }

    #[test]
    fn test_summary_serializes() {
        let summary = Summary {
            time_range: (None, None),
            total_lines: 0,
            suppressed_lines: 0,
            error_count: 0,
            warning_count: 0,
            pods: vec![],
            top_errors: vec![],
            top_warnings: vec![],
            timeline: vec![],
            novel_patterns: vec![],
            restart_events: vec![],
        };

        let json = serde_json::to_string(&summary).expect("serialize empty summary");
        assert!(json.contains("\"total_lines\":0"));
    }

    #[test]
    fn test_pipeline_output_has_hint() {
        let output = PipelineOutput {
            hint: "Test hint".to_string(),
            summary: Summary {
                time_range: (None, None),
                total_lines: 100,
                suppressed_lines: 90,
                error_count: 5,
                warning_count: 5,
                pods: vec![],
                top_errors: vec![],
                top_warnings: vec![],
                timeline: vec![],
                novel_patterns: vec![],
                restart_events: vec![],
            },
            lines: vec![],
        };

        let json = serde_json::to_string(&output).expect("serialize");
        assert!(json.contains("\"_hint\":\"Test hint\""));
    }
}
