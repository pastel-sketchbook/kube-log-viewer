//! Filter stage for the classify-filter-reduce pipeline.
//!
//! Takes classified log lines and applies a [`FilterConfig`] to determine which
//! lines pass through. Repeated lines that are suppressed get collapsed into
//! [`CollapsedGroup`] summaries rather than being silently dropped — this
//! preserves the information that repetition occurred.
//!
//! The filter produces [`FilteredItem`]s: either individual [`ClassifiedLine`]s
//! or [`CollapsedGroup`] placeholders.

use crate::types::{ClassifiedLine, CollapsedGroup, FilterConfig, LineClass};

// ---------------------------------------------------------------------------
// Output item
// ---------------------------------------------------------------------------

/// A single item produced by the filter stage.
#[derive(Debug, Clone)]
pub enum FilteredItem {
    /// A classified line that passed the filter.
    Line(ClassifiedLine),
    /// A collapsed summary of suppressed repeated lines.
    Collapsed(CollapsedGroup),
}

// ---------------------------------------------------------------------------
// Filter stats
// ---------------------------------------------------------------------------

/// Statistics collected during filtering.
#[derive(Debug, Clone, Default)]
pub struct FilterStats {
    /// Total lines received (before filtering).
    pub total: u64,
    /// Lines that passed the filter.
    pub kept: u64,
    /// Lines suppressed (dropped or collapsed).
    pub suppressed: u64,
    /// Number of collapsed groups emitted.
    pub collapsed_groups: u64,
}

// ---------------------------------------------------------------------------
// Pending collapse accumulator
// ---------------------------------------------------------------------------

/// Accumulates consecutive repeated lines sharing the same canonical form
/// so they can be emitted as a single [`CollapsedGroup`].
struct PendingCollapse {
    /// The canonical form being accumulated.
    canonical: String,
    /// Running count (sum of individual `Repeated.count` values).
    count: u32,
    /// One verbatim example (the raw text of the first repeated line seen).
    sample: String,
}

impl PendingCollapse {
    fn new(canonical: String, count: u32, sample: String) -> Self {
        Self {
            canonical,
            count,
            sample,
        }
    }

    /// Convert into a [`CollapsedGroup`].
    fn into_group(self) -> CollapsedGroup {
        CollapsedGroup {
            collapsed: true,
            count: self.count,
            canonical: self.canonical,
            sample: self.sample,
        }
    }
}

// ---------------------------------------------------------------------------
// Filter function
// ---------------------------------------------------------------------------

/// Filter a sequence of classified lines according to the given config.
///
/// - Lines whose class is included by `config` pass through as
///   [`FilteredItem::Line`].
/// - `Repeated` lines that are *not* included are accumulated: consecutive
///   runs with the same canonical form are collapsed into a single
///   [`FilteredItem::Collapsed`] group.
/// - Other suppressed classes (HealthCheck, Normal) are silently dropped.
///
/// Returns the filtered items and statistics about what was kept/suppressed.
pub fn filter(
    lines: Vec<ClassifiedLine>,
    config: &FilterConfig,
) -> (Vec<FilteredItem>, FilterStats) {
    let mut items = Vec::new();
    let mut stats = FilterStats::default();
    let mut pending: Option<PendingCollapse> = None;

    for line in lines {
        stats.total += 1;

        if config.should_include(&line.class) {
            // Flush any pending collapse before emitting a kept line.
            if let Some(p) = pending.take() {
                stats.collapsed_groups += 1;
                items.push(FilteredItem::Collapsed(p.into_group()));
            }
            stats.kept += 1;
            items.push(FilteredItem::Line(line));
        } else {
            // Suppressed line.
            stats.suppressed += 1;

            // If it's a Repeated line, try to accumulate into a collapse group.
            if let LineClass::Repeated {
                count: _,
                canonical,
                ..
            } = &line.class
            {
                match &mut pending {
                    Some(p) if p.canonical == *canonical => {
                        // Same canonical form — merge into the pending group.
                        // Use the latest count (from Classifier, which tracks
                        // the running total), but for the collapsed group we
                        // just need to count how many individual lines were
                        // suppressed, so we increment by 1.
                        p.count += 1;
                    }
                    _ => {
                        // Different canonical form or no pending group.
                        // Flush previous if any.
                        if let Some(p) = pending.take() {
                            stats.collapsed_groups += 1;
                            items.push(FilteredItem::Collapsed(p.into_group()));
                        }
                        pending =
                            Some(PendingCollapse::new(canonical.clone(), 1, line.raw.clone()));
                    }
                }
            }
            // Non-Repeated suppressed lines (HealthCheck, Normal) are just
            // dropped — no collapse group needed.
        }
    }

    // Flush trailing pending collapse.
    if let Some(p) = pending.take() {
        stats.collapsed_groups += 1;
        items.push(FilteredItem::Collapsed(p.into_group()));
    }

    (items, stats)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;

    /// Helper to build a ClassifiedLine with minimal boilerplate.
    fn make_line(class: LineClass, raw: &str) -> ClassifiedLine {
        ClassifiedLine {
            timestamp: Some(Timestamp::now()),
            pod: "pod-1".to_string(),
            container: None,
            class,
            level: None,
            msg: None,
            raw: raw.to_string(),
            fields: None,
        }
    }

    // -- Basic pass-through -------------------------------------------------

    #[test]
    fn test_troubleshoot_keeps_errors_warnings_lifecycle_novel() {
        let lines = vec![
            make_line(LineClass::Error, "ERROR something broke"),
            make_line(LineClass::Warning, "WARN timeout"),
            make_line(LineClass::Lifecycle, "container started"),
            make_line(LineClass::Novel, "first-seen pattern"),
        ];

        let (items, stats) = filter(lines, &FilterConfig::troubleshoot());
        assert_eq!(stats.total, 4);
        assert_eq!(stats.kept, 4);
        assert_eq!(stats.suppressed, 0);
        assert_eq!(items.len(), 4);
        assert!(items.iter().all(|i| matches!(i, FilteredItem::Line(_))));
    }

    #[test]
    fn test_troubleshoot_drops_healthcheck_and_normal() {
        let lines = vec![
            make_line(LineClass::HealthCheck, "GET /healthz 200"),
            make_line(LineClass::Normal, "INFO all good"),
            make_line(LineClass::Error, "ERROR panic"),
        ];

        let (items, stats) = filter(lines, &FilterConfig::troubleshoot());
        assert_eq!(stats.total, 3);
        assert_eq!(stats.kept, 1);
        assert_eq!(stats.suppressed, 2);
        // Only the error line passes through.
        assert_eq!(items.len(), 1);
        match &items[0] {
            FilteredItem::Line(l) => assert_eq!(l.class, LineClass::Error),
            _ => panic!("expected Line"),
        }
    }

    // -- Collapse repeated lines --------------------------------------------

    #[test]
    fn test_repeated_lines_collapsed_in_troubleshoot() {
        let lines = vec![
            make_line(
                LineClass::Repeated {
                    count: 2,
                    canonical: "INFO GET /api/users request_id=<num> latency=<num>ms".into(),
                },
                "INFO GET /api/users request_id=123 latency=12ms",
            ),
            make_line(
                LineClass::Repeated {
                    count: 3,
                    canonical: "INFO GET /api/users request_id=<num> latency=<num>ms".into(),
                },
                "INFO GET /api/users request_id=456 latency=14ms",
            ),
            make_line(
                LineClass::Repeated {
                    count: 4,
                    canonical: "INFO GET /api/users request_id=<num> latency=<num>ms".into(),
                },
                "INFO GET /api/users request_id=789 latency=9ms",
            ),
        ];

        let (items, stats) = filter(lines, &FilterConfig::troubleshoot());
        assert_eq!(stats.total, 3);
        assert_eq!(stats.suppressed, 3);
        assert_eq!(stats.kept, 0);
        assert_eq!(stats.collapsed_groups, 1);
        assert_eq!(items.len(), 1);
        match &items[0] {
            FilteredItem::Collapsed(g) => {
                assert!(g.collapsed);
                assert_eq!(g.count, 3); // 3 lines accumulated
                assert!(g.canonical.contains("/api/users"));
                assert!(g.sample.contains("request_id=123")); // first sample preserved
            }
            _ => panic!("expected Collapsed"),
        }
    }

    #[test]
    fn test_different_canonical_forms_produce_separate_groups() {
        let lines = vec![
            make_line(
                LineClass::Repeated {
                    count: 2,
                    canonical: "pattern-A".into(),
                },
                "raw pattern A",
            ),
            make_line(
                LineClass::Repeated {
                    count: 2,
                    canonical: "pattern-A".into(),
                },
                "raw pattern A v2",
            ),
            make_line(
                LineClass::Repeated {
                    count: 2,
                    canonical: "pattern-B".into(),
                },
                "raw pattern B",
            ),
            make_line(
                LineClass::Repeated {
                    count: 3,
                    canonical: "pattern-B".into(),
                },
                "raw pattern B v2",
            ),
        ];

        let (items, stats) = filter(lines, &FilterConfig::troubleshoot());
        assert_eq!(stats.total, 4);
        assert_eq!(stats.suppressed, 4);
        assert_eq!(stats.collapsed_groups, 2);
        assert_eq!(items.len(), 2);

        // First group: pattern-A with count 2
        match &items[0] {
            FilteredItem::Collapsed(g) => {
                assert_eq!(g.canonical, "pattern-A");
                assert_eq!(g.count, 2);
            }
            _ => panic!("expected Collapsed for pattern-A"),
        }

        // Second group: pattern-B with count 2
        match &items[1] {
            FilteredItem::Collapsed(g) => {
                assert_eq!(g.canonical, "pattern-B");
                assert_eq!(g.count, 2);
            }
            _ => panic!("expected Collapsed for pattern-B"),
        }
    }

    #[test]
    fn test_interleaved_repeated_and_kept_lines() {
        let lines = vec![
            make_line(
                LineClass::Repeated {
                    count: 2,
                    canonical: "pattern-A".into(),
                },
                "repeated A 1",
            ),
            make_line(
                LineClass::Repeated {
                    count: 3,
                    canonical: "pattern-A".into(),
                },
                "repeated A 2",
            ),
            make_line(LineClass::Error, "ERROR boom"),
            make_line(
                LineClass::Repeated {
                    count: 2,
                    canonical: "pattern-A".into(),
                },
                "repeated A 3",
            ),
        ];

        let (items, stats) = filter(lines, &FilterConfig::troubleshoot());
        assert_eq!(stats.total, 4);
        assert_eq!(stats.kept, 1);
        assert_eq!(stats.suppressed, 3);
        // Two collapsed groups (split by the error line) + 1 error line.
        assert_eq!(stats.collapsed_groups, 2);
        assert_eq!(items.len(), 3);

        match &items[0] {
            FilteredItem::Collapsed(g) => {
                assert_eq!(g.canonical, "pattern-A");
                assert_eq!(g.count, 2);
            }
            _ => panic!("expected Collapsed"),
        }
        match &items[1] {
            FilteredItem::Line(l) => assert_eq!(l.class, LineClass::Error),
            _ => panic!("expected Line"),
        }
        match &items[2] {
            FilteredItem::Collapsed(g) => {
                assert_eq!(g.canonical, "pattern-A");
                assert_eq!(g.count, 1);
            }
            _ => panic!("expected Collapsed"),
        }
    }

    // -- All mode passes everything through ---------------------------------

    #[test]
    fn test_all_mode_passes_everything() {
        let lines = vec![
            make_line(LineClass::Error, "ERROR"),
            make_line(LineClass::HealthCheck, "GET /healthz"),
            make_line(LineClass::Normal, "INFO ok"),
            make_line(
                LineClass::Repeated {
                    count: 5,
                    canonical: "pat".into(),
                },
                "repeated",
            ),
        ];

        let (items, stats) = filter(lines, &FilterConfig::all());
        assert_eq!(stats.total, 4);
        assert_eq!(stats.kept, 4);
        assert_eq!(stats.suppressed, 0);
        assert_eq!(stats.collapsed_groups, 0);
        assert_eq!(items.len(), 4);
        assert!(items.iter().all(|i| matches!(i, FilteredItem::Line(_))));
    }

    // -- Verbose mode -------------------------------------------------------

    #[test]
    fn test_verbose_keeps_normal_but_drops_healthcheck() {
        let lines = vec![
            make_line(LineClass::Normal, "INFO ok"),
            make_line(LineClass::HealthCheck, "kube-probe"),
        ];

        let (items, stats) = filter(lines, &FilterConfig::verbose());
        assert_eq!(stats.total, 2);
        assert_eq!(stats.kept, 1);
        assert_eq!(stats.suppressed, 1);
        assert_eq!(items.len(), 1);
        match &items[0] {
            FilteredItem::Line(l) => assert_eq!(l.class, LineClass::Normal),
            _ => panic!("expected Normal line"),
        }
    }

    // -- Custom include list ------------------------------------------------

    #[test]
    fn test_custom_include_list() {
        let lines = vec![
            make_line(LineClass::Error, "ERROR"),
            make_line(LineClass::Warning, "WARN"),
            make_line(LineClass::Lifecycle, "started"),
            make_line(LineClass::Novel, "new pattern"),
            make_line(LineClass::Normal, "INFO ok"),
        ];

        let config = FilterConfig::from_include_list("error,novel");
        let (items, stats) = filter(lines, &config);
        assert_eq!(stats.total, 5);
        assert_eq!(stats.kept, 2);
        assert_eq!(stats.suppressed, 3);
        assert_eq!(items.len(), 2);
    }

    // -- Empty input --------------------------------------------------------

    #[test]
    fn test_empty_input() {
        let (items, stats) = filter(vec![], &FilterConfig::troubleshoot());
        assert_eq!(stats.total, 0);
        assert_eq!(stats.kept, 0);
        assert_eq!(stats.suppressed, 0);
        assert_eq!(stats.collapsed_groups, 0);
        assert!(items.is_empty());
    }

    // -- Single repeated line -----------------------------------------------

    #[test]
    fn test_single_repeated_line_becomes_collapsed() {
        let lines = vec![make_line(
            LineClass::Repeated {
                count: 2,
                canonical: "single-pat".into(),
            },
            "one repeated line",
        )];

        let (items, stats) = filter(lines, &FilterConfig::troubleshoot());
        assert_eq!(stats.total, 1);
        assert_eq!(stats.suppressed, 1);
        assert_eq!(stats.collapsed_groups, 1);
        assert_eq!(items.len(), 1);
        match &items[0] {
            FilteredItem::Collapsed(g) => {
                assert_eq!(g.count, 1);
                assert_eq!(g.canonical, "single-pat");
            }
            _ => panic!("expected Collapsed"),
        }
    }

    // -- Mixed healthcheck + normal suppressed without collapse --------------

    #[test]
    fn test_healthcheck_and_normal_suppressed_no_collapse() {
        let lines = vec![
            make_line(LineClass::HealthCheck, "GET /healthz 200"),
            make_line(LineClass::Normal, "INFO routine"),
            make_line(LineClass::HealthCheck, "GET /readyz 200"),
            make_line(LineClass::Normal, "INFO another routine"),
        ];

        let (items, stats) = filter(lines, &FilterConfig::troubleshoot());
        assert_eq!(stats.total, 4);
        assert_eq!(stats.suppressed, 4);
        assert_eq!(stats.kept, 0);
        // No collapsed groups — only Repeated lines produce collapse groups.
        assert_eq!(stats.collapsed_groups, 0);
        assert!(items.is_empty());
    }

    // -- Collapse group sample preserves first raw line ---------------------

    #[test]
    fn test_collapse_preserves_first_sample() {
        let lines = vec![
            make_line(
                LineClass::Repeated {
                    count: 2,
                    canonical: "pat".into(),
                },
                "first raw line",
            ),
            make_line(
                LineClass::Repeated {
                    count: 3,
                    canonical: "pat".into(),
                },
                "second raw line",
            ),
            make_line(
                LineClass::Repeated {
                    count: 4,
                    canonical: "pat".into(),
                },
                "third raw line",
            ),
        ];

        let (items, _) = filter(lines, &FilterConfig::troubleshoot());
        assert_eq!(items.len(), 1);
        match &items[0] {
            FilteredItem::Collapsed(g) => {
                assert_eq!(g.sample, "first raw line");
                assert_eq!(g.count, 3);
            }
            _ => panic!("expected Collapsed"),
        }
    }
}
