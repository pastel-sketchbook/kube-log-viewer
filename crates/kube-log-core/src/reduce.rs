//! Reduce stage for the classify-filter-reduce pipeline.
//!
//! Takes classified log lines and compresses them into a bounded [`Summary`]
//! regardless of input volume. This is critical for token economy: a pod
//! producing 10,000 lines/minute can easily overwhelm an LLM context window.
//!
//! The reduce output has bounded size:
//! - `top_errors`: capped at [`MAX_ERROR_BUCKETS`] (20)
//! - `top_warnings`: capped at [`MAX_WARNING_BUCKETS`] (20)
//! - `timeline`: 1-minute buckets, capped at [`MAX_TIMELINE_BUCKETS`] (60)
//! - `novel_patterns`: capped at [`MAX_NOVEL_PATTERNS`] (30)
//!
//! This guarantees the summary fits within ~2K-4K tokens even for pods
//! producing millions of lines.

use std::collections::HashMap;

use jiff::Timestamp;

use crate::classify::normalize;
use crate::filter::FilterStats;
use crate::types::{ClassifiedLine, ErrorBucket, LineClass, RestartEvent, Summary, TimelineEntry};

// ---------------------------------------------------------------------------
// Caps (from ADR-0007)
// ---------------------------------------------------------------------------

/// Maximum deduplicated error buckets in the summary.
const MAX_ERROR_BUCKETS: usize = 20;

/// Maximum deduplicated warning buckets in the summary.
const MAX_WARNING_BUCKETS: usize = 20;

/// Maximum 1-minute timeline entries (last hour).
const MAX_TIMELINE_BUCKETS: usize = 60;

/// Maximum novel patterns to report.
const MAX_NOVEL_PATTERNS: usize = 30;

// ---------------------------------------------------------------------------
// Restart detection patterns
// ---------------------------------------------------------------------------

/// Patterns in log lines that indicate a pod restart.
const RESTART_PATTERNS: &[&str] = &[
    "CrashLoopBackOff",
    "BackOff",
    "restarting",
    "Restarting",
    "OOMKilled",
    "container killed",
];

// ---------------------------------------------------------------------------
// Internal accumulators
// ---------------------------------------------------------------------------

/// Accumulator for error/warning buckets during the reduce phase.
struct BucketAccumulator {
    /// canonical → (count, first_seen, last_seen, sample)
    buckets: HashMap<String, (u64, Timestamp, Timestamp, String)>,
}

impl BucketAccumulator {
    fn new() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }

    fn add(&mut self, line: &ClassifiedLine) {
        let canonical = normalize(&line.raw);
        let ts = line.timestamp.unwrap_or_else(Timestamp::now);

        self.buckets
            .entry(canonical)
            .and_modify(|(count, first, last, _sample)| {
                *count += 1;
                if ts < *first {
                    *first = ts;
                }
                if ts > *last {
                    *last = ts;
                }
            })
            .or_insert((1, ts, ts, line.raw.clone()));
    }

    /// Drain into a sorted Vec<ErrorBucket>, capped at `limit`.
    fn into_sorted(self, limit: usize) -> Vec<ErrorBucket> {
        let mut buckets: Vec<ErrorBucket> = self
            .buckets
            .into_iter()
            .map(
                |(canonical, (count, first_seen, last_seen, sample))| ErrorBucket {
                    canonical,
                    count,
                    first_seen,
                    last_seen,
                    sample,
                },
            )
            .collect();

        // Sort by count descending, then by first_seen ascending for stability.
        buckets.sort_by(|a, b| b.count.cmp(&a.count).then(a.first_seen.cmp(&b.first_seen)));
        buckets.truncate(limit);
        buckets
    }
}

/// Accumulator for 1-minute timeline buckets.
struct TimelineAccumulator {
    /// bucket_start → (error_count, warning_count, novel_count)
    buckets: HashMap<Timestamp, (u64, u64, u64)>,
}

impl TimelineAccumulator {
    fn new() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }

    fn add(&mut self, line: &ClassifiedLine) {
        let ts = match line.timestamp {
            Some(t) => t,
            None => return, // Skip lines without timestamps for timeline.
        };

        // Truncate to minute boundary.
        let bucket_start = truncate_to_minute(ts);
        let entry = self.buckets.entry(bucket_start).or_insert((0, 0, 0));

        match &line.class {
            LineClass::Error => entry.0 += 1,
            LineClass::Warning => entry.1 += 1,
            LineClass::Novel => entry.2 += 1,
            _ => {}
        }
    }

    /// Drain into a sorted Vec<TimelineEntry>, keeping only the last `limit`
    /// entries (most recent).
    fn into_sorted(self, limit: usize) -> Vec<TimelineEntry> {
        let mut entries: Vec<TimelineEntry> = self
            .buckets
            .into_iter()
            .map(
                |(bucket_start, (error_count, warning_count, novel_count))| TimelineEntry {
                    bucket_start,
                    error_count,
                    warning_count,
                    novel_count,
                },
            )
            .collect();

        // Sort by time ascending.
        entries.sort_by_key(|e| e.bucket_start);

        // Keep only the last `limit` entries (most recent hour).
        if entries.len() > limit {
            entries = entries.split_off(entries.len() - limit);
        }
        entries
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Reduce a sequence of classified lines into a bounded [`Summary`].
///
/// `lines` should be the *full* set of classified lines (before filtering),
/// so that counts are accurate. `filter_stats` provides the suppressed count
/// from the filter stage.
pub fn reduce(lines: &[ClassifiedLine], filter_stats: &FilterStats) -> Summary {
    let mut errors = BucketAccumulator::new();
    let mut warnings = BucketAccumulator::new();
    let mut timeline = TimelineAccumulator::new();
    let mut novel_patterns: Vec<String> = Vec::new();
    let mut restart_events: Vec<RestartEvent> = Vec::new();

    let mut error_count: u64 = 0;
    let mut warning_count: u64 = 0;
    let mut first_ts: Option<Timestamp> = None;
    let mut last_ts: Option<Timestamp> = None;

    for line in lines {
        // Track time range.
        if let Some(ts) = line.timestamp {
            first_ts = Some(match first_ts {
                Some(f) if ts < f => ts,
                Some(f) => f,
                None => ts,
            });
            last_ts = Some(match last_ts {
                Some(l) if ts > l => ts,
                Some(l) => l,
                None => ts,
            });
        }

        // Update timeline for errors/warnings/novel.
        timeline.add(line);

        match &line.class {
            LineClass::Error => {
                error_count += 1;
                errors.add(line);
            }
            LineClass::Warning => {
                warning_count += 1;
                warnings.add(line);
            }
            LineClass::Novel => {
                if novel_patterns.len() < MAX_NOVEL_PATTERNS {
                    // Store the message or raw line as the pattern.
                    let pattern = line.msg.as_deref().unwrap_or(&line.raw).to_string();
                    novel_patterns.push(pattern);
                }
            }
            LineClass::Lifecycle => {
                // Check for restart indicators.
                let text = line.msg.as_deref().unwrap_or(&line.raw);
                if RESTART_PATTERNS.iter().any(|p| text.contains(p)) {
                    let ts = line.timestamp.unwrap_or_else(Timestamp::now);
                    // Extract reason: use the first matching pattern.
                    let reason = RESTART_PATTERNS
                        .iter()
                        .find(|p| text.contains(*p))
                        .unwrap_or(&"restart")
                        .to_string();
                    restart_events.push(RestartEvent {
                        pod: line.pod.clone(),
                        at: ts,
                        reason,
                    });
                }
            }
            _ => {}
        }
    }

    Summary {
        time_range: (first_ts, last_ts),
        total_lines: filter_stats.total,
        suppressed_lines: filter_stats.suppressed,
        error_count,
        warning_count,
        pods: vec![], // Populated by the caller with live PodInfo data.
        top_errors: errors.into_sorted(MAX_ERROR_BUCKETS),
        top_warnings: warnings.into_sorted(MAX_WARNING_BUCKETS),
        timeline: timeline.into_sorted(MAX_TIMELINE_BUCKETS),
        novel_patterns,
        restart_events,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Truncate a timestamp to the start of its minute.
fn truncate_to_minute(ts: Timestamp) -> Timestamp {
    let secs = ts.as_second() - (ts.as_second() % 60);
    Timestamp::from_second(secs).unwrap_or(ts)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::SignedDuration;

    /// Helper to make a ClassifiedLine with a specific timestamp offset.
    fn make_line_at(class: LineClass, raw: &str, minutes_offset: i64) -> ClassifiedLine {
        let base: Timestamp = "2026-03-06T10:00:00Z".parse().unwrap();
        let ts = base
            .checked_add(SignedDuration::from_secs(minutes_offset * 60))
            .unwrap();
        ClassifiedLine {
            timestamp: Some(ts),
            pod: "pod-1".to_string(),
            container: None,
            class,
            level: None,
            msg: Some(raw.to_string()),
            raw: raw.to_string(),
            fields: None,
        }
    }

    fn make_line(class: LineClass, raw: &str) -> ClassifiedLine {
        make_line_at(class, raw, 0)
    }

    fn default_stats(total: u64, suppressed: u64) -> FilterStats {
        FilterStats {
            total,
            kept: total - suppressed,
            suppressed,
            collapsed_groups: 0,
        }
    }

    // -- Basic counting -----------------------------------------------------

    #[test]
    fn test_counts_errors_and_warnings() {
        let lines = vec![
            make_line(LineClass::Error, "ERROR db down"),
            make_line(LineClass::Error, "ERROR db down again"),
            make_line(LineClass::Warning, "WARN timeout"),
            make_line(LineClass::Novel, "new pattern"),
        ];

        let summary = reduce(&lines, &default_stats(100, 96));
        assert_eq!(summary.error_count, 2);
        assert_eq!(summary.warning_count, 1);
        assert_eq!(summary.total_lines, 100);
        assert_eq!(summary.suppressed_lines, 96);
    }

    // -- Time range ---------------------------------------------------------

    #[test]
    fn test_time_range_tracks_first_and_last() {
        let lines = vec![
            make_line_at(LineClass::Error, "err1", 5),
            make_line_at(LineClass::Error, "err2", 1),
            make_line_at(LineClass::Error, "err3", 10),
        ];

        let summary = reduce(&lines, &default_stats(3, 0));
        let (first, last) = summary.time_range;
        assert!(first.is_some());
        assert!(last.is_some());
        // First should be minute 1, last should be minute 10.
        let base: Timestamp = "2026-03-06T10:00:00Z".parse().unwrap();
        assert_eq!(
            first.unwrap(),
            base.checked_add(SignedDuration::from_secs(60)).unwrap()
        );
        assert_eq!(
            last.unwrap(),
            base.checked_add(SignedDuration::from_secs(10 * 60))
                .unwrap()
        );
    }

    #[test]
    fn test_time_range_none_when_no_timestamps() {
        let mut line = make_line(LineClass::Error, "err");
        line.timestamp = None;

        let summary = reduce(&[line], &default_stats(1, 0));
        assert!(summary.time_range.0.is_none());
        assert!(summary.time_range.1.is_none());
    }

    // -- Top errors dedup ---------------------------------------------------

    #[test]
    fn test_top_errors_deduplicates_by_canonical_form() {
        // Two lines that normalize to the same canonical form.
        let lines = vec![
            make_line_at(LineClass::Error, "ERROR connection refused to db:5432", 1),
            make_line_at(LineClass::Error, "ERROR connection refused to db:3306", 2),
        ];

        let summary = reduce(&lines, &default_stats(2, 0));
        // Both should collapse into one error bucket (port numbers normalize).
        assert_eq!(summary.top_errors.len(), 1);
        assert_eq!(summary.top_errors[0].count, 2);
    }

    #[test]
    fn test_top_errors_sorted_by_frequency() {
        let lines = vec![
            make_line(LineClass::Error, "ERROR rare problem xyz"),
            make_line(LineClass::Error, "ERROR common problem abc"),
            make_line(LineClass::Error, "ERROR common problem abc"),
            make_line(LineClass::Error, "ERROR common problem abc"),
        ];

        let summary = reduce(&lines, &default_stats(4, 0));
        assert!(summary.top_errors.len() >= 2);
        // Most frequent first.
        assert!(summary.top_errors[0].count >= summary.top_errors[1].count);
    }

    #[test]
    fn test_top_errors_capped_at_max() {
        // Create 25 distinct error patterns.
        let lines: Vec<ClassifiedLine> = (0..25)
            .map(|i| {
                make_line(
                    LineClass::Error,
                    &format!("ERROR unique_pattern_{}", (b'a' + i) as char),
                )
            })
            .collect();

        let summary = reduce(&lines, &default_stats(25, 0));
        assert!(summary.top_errors.len() <= MAX_ERROR_BUCKETS);
    }

    // -- Top warnings -------------------------------------------------------

    #[test]
    fn test_top_warnings_populated() {
        let lines = vec![
            make_line(LineClass::Warning, "WARN timeout connecting"),
            make_line(LineClass::Warning, "WARN timeout connecting"),
            make_line(LineClass::Warning, "WARN deprecated API"),
        ];

        let summary = reduce(&lines, &default_stats(3, 0));
        assert!(!summary.top_warnings.is_empty());
        // First bucket should be the most frequent.
        assert!(summary.top_warnings[0].count >= 1);
    }

    // -- Timeline -----------------------------------------------------------

    #[test]
    fn test_timeline_groups_by_minute() {
        let lines = vec![
            make_line_at(LineClass::Error, "err1", 0),
            make_line_at(LineClass::Error, "err2", 0), // same minute
            make_line_at(LineClass::Warning, "warn1", 1),
            make_line_at(LineClass::Novel, "novel1", 2),
        ];

        let summary = reduce(&lines, &default_stats(4, 0));
        // 3 distinct minutes: 0, 1, 2.
        assert_eq!(summary.timeline.len(), 3);
        // Sorted by time.
        assert!(summary.timeline[0].bucket_start <= summary.timeline[1].bucket_start);
        assert!(summary.timeline[1].bucket_start <= summary.timeline[2].bucket_start);
        // Minute 0: 2 errors.
        assert_eq!(summary.timeline[0].error_count, 2);
        // Minute 1: 1 warning.
        assert_eq!(summary.timeline[1].warning_count, 1);
        // Minute 2: 1 novel.
        assert_eq!(summary.timeline[2].novel_count, 1);
    }

    #[test]
    fn test_timeline_capped_keeps_most_recent() {
        // Create 70 entries (one per minute), exceeding the 60 cap.
        let lines: Vec<ClassifiedLine> = (0..70)
            .map(|i| make_line_at(LineClass::Error, "err", i))
            .collect();

        let summary = reduce(&lines, &default_stats(70, 0));
        assert!(summary.timeline.len() <= MAX_TIMELINE_BUCKETS);
        // The earliest entries should be dropped, keeping minutes 10-69.
        let base: Timestamp = "2026-03-06T10:00:00Z".parse().unwrap();
        let expected_first = truncate_to_minute(
            base.checked_add(SignedDuration::from_secs(10 * 60))
                .unwrap(),
        );
        assert_eq!(summary.timeline[0].bucket_start, expected_first);
    }

    #[test]
    fn test_timeline_skips_lines_without_timestamps() {
        let mut line = make_line(LineClass::Error, "err");
        line.timestamp = None;

        let summary = reduce(&[line], &default_stats(1, 0));
        assert!(summary.timeline.is_empty());
    }

    // -- Novel patterns -----------------------------------------------------

    #[test]
    fn test_novel_patterns_collected() {
        let lines = vec![
            make_line(LineClass::Novel, "first-seen pattern A"),
            make_line(LineClass::Novel, "first-seen pattern B"),
        ];

        let summary = reduce(&lines, &default_stats(2, 0));
        assert_eq!(summary.novel_patterns.len(), 2);
        assert!(
            summary
                .novel_patterns
                .contains(&"first-seen pattern A".to_string())
        );
        assert!(
            summary
                .novel_patterns
                .contains(&"first-seen pattern B".to_string())
        );
    }

    #[test]
    fn test_novel_patterns_capped() {
        let lines: Vec<ClassifiedLine> = (0..40)
            .map(|i| make_line(LineClass::Novel, &format!("pattern {}", i)))
            .collect();

        let summary = reduce(&lines, &default_stats(40, 0));
        assert!(summary.novel_patterns.len() <= MAX_NOVEL_PATTERNS);
    }

    // -- Restart events -----------------------------------------------------

    #[test]
    fn test_restart_events_detected() {
        let lines = vec![
            make_line_at(
                LineClass::Lifecycle,
                "CrashLoopBackOff restarting container",
                5,
            ),
            make_line_at(LineClass::Lifecycle, "container started", 6), // not a restart
        ];

        let summary = reduce(&lines, &default_stats(2, 0));
        assert_eq!(summary.restart_events.len(), 1);
        assert_eq!(summary.restart_events[0].reason, "CrashLoopBackOff");
        assert_eq!(summary.restart_events[0].pod, "pod-1");
    }

    #[test]
    fn test_oomkilled_detected_as_restart() {
        let lines = vec![make_line(LineClass::Lifecycle, "container OOMKilled")];

        let summary = reduce(&lines, &default_stats(1, 0));
        assert_eq!(summary.restart_events.len(), 1);
        assert_eq!(summary.restart_events[0].reason, "OOMKilled");
    }

    // -- Empty input --------------------------------------------------------

    #[test]
    fn test_empty_input() {
        let summary = reduce(&[], &default_stats(0, 0));
        assert_eq!(summary.total_lines, 0);
        assert_eq!(summary.error_count, 0);
        assert_eq!(summary.warning_count, 0);
        assert!(summary.top_errors.is_empty());
        assert!(summary.top_warnings.is_empty());
        assert!(summary.timeline.is_empty());
        assert!(summary.novel_patterns.is_empty());
        assert!(summary.restart_events.is_empty());
    }

    // -- Pods field is empty (populated by caller) --------------------------

    #[test]
    fn test_pods_empty_by_default() {
        let lines = vec![make_line(LineClass::Error, "err")];
        let summary = reduce(&lines, &default_stats(1, 0));
        assert!(summary.pods.is_empty());
    }

    // -- ErrorBucket fields -------------------------------------------------

    #[test]
    fn test_error_bucket_tracks_first_and_last_seen() {
        let lines = vec![
            make_line_at(LineClass::Error, "ERROR same pattern", 1),
            make_line_at(LineClass::Error, "ERROR same pattern", 5),
            make_line_at(LineClass::Error, "ERROR same pattern", 3),
        ];

        let summary = reduce(&lines, &default_stats(3, 0));
        assert_eq!(summary.top_errors.len(), 1);
        let bucket = &summary.top_errors[0];
        assert_eq!(bucket.count, 3);
        let base: Timestamp = "2026-03-06T10:00:00Z".parse().unwrap();
        assert_eq!(
            bucket.first_seen,
            base.checked_add(SignedDuration::from_secs(60)).unwrap()
        );
        assert_eq!(
            bucket.last_seen,
            base.checked_add(SignedDuration::from_secs(5 * 60)).unwrap()
        );
    }

    // -- truncate_to_minute -------------------------------------------------

    #[test]
    fn test_truncate_to_minute() {
        let ts: Timestamp = "2026-03-06T10:15:47Z".parse().unwrap();
        let truncated = truncate_to_minute(ts);
        let expected: Timestamp = "2026-03-06T10:15:00Z".parse().unwrap();
        assert_eq!(truncated, expected);
    }

    #[test]
    fn test_truncate_to_minute_already_on_boundary() {
        let ts: Timestamp = "2026-03-06T10:15:00Z".parse().unwrap();
        assert_eq!(truncate_to_minute(ts), ts);
    }
}
