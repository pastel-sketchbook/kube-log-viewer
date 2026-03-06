//! Export stage for the classify-filter-reduce pipeline.
//!
//! Formats the pipeline output into one of three formats:
//!
//! - **JSON** — Single JSON document with `_hint`, `summary`, and `lines`.
//!   This is the default CLI output, designed for LLM consumption.
//! - **JSONL** — One JSON object per line (streaming-friendly). Each line
//!   is either a [`ClassifiedLine`] or a [`CollapsedGroup`].
//! - **Plain** — Human-readable text fallback for `grep`-style workflows.

use std::io::{self, Write};

use crate::filter::FilteredItem;
use crate::types::{ClassifiedLine, CollapsedGroup, OutputFormat, PipelineOutput, Summary};

// ---------------------------------------------------------------------------
// Hint generation
// ---------------------------------------------------------------------------

/// Generate the `_hint` field for LLM consumption.
///
/// Describes what data this is, what was filtered, and the scale of reduction.
pub fn generate_hint(context: &str, namespace: &str, summary: &Summary) -> String {
    let mode = "Troubleshoot mode: only errors, warnings, lifecycle events, and novel patterns shown. Health checks and repeated lines suppressed.";
    format!(
        "Kubernetes pod logs from context '{}', namespace '{}'. {} {} of {} lines omitted.",
        context, namespace, mode, summary.suppressed_lines, summary.total_lines,
    )
}

// ---------------------------------------------------------------------------
// JSON export (single document)
// ---------------------------------------------------------------------------

/// Export the full pipeline output as a single pretty-printed JSON document.
///
/// This is the default output format. The structure matches [`PipelineOutput`]:
/// ```json
/// { "_hint": "...", "summary": {...}, "lines": [...] }
/// ```
pub fn export_json<W: Write>(writer: &mut W, output: &PipelineOutput) -> io::Result<()> {
    let json = serde_json::to_string_pretty(output).map_err(io::Error::other)?;
    writeln!(writer, "{}", json)
}

/// Export the full pipeline output as a compact (non-pretty) JSON document.
pub fn export_json_compact<W: Write>(writer: &mut W, output: &PipelineOutput) -> io::Result<()> {
    let json = serde_json::to_string(output).map_err(io::Error::other)?;
    writeln!(writer, "{}", json)
}

// ---------------------------------------------------------------------------
// JSONL export (streaming)
// ---------------------------------------------------------------------------

/// Export filtered items as JSON-lines (one JSON object per line).
///
/// Each line is either a serialized [`ClassifiedLine`] or a
/// [`CollapsedGroup`]. This format is streaming-friendly and can be piped
/// incrementally into consumers.
pub fn export_jsonl<W: Write>(writer: &mut W, items: &[FilteredItem]) -> io::Result<()> {
    for item in items {
        let json = match item {
            FilteredItem::Line(line) => serde_json::to_string(line),
            FilteredItem::Collapsed(group) => serde_json::to_string(group),
        };
        let json = json.map_err(io::Error::other)?;
        writeln!(writer, "{}", json)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Plain text export
// ---------------------------------------------------------------------------

/// Export filtered items as plain text (human-readable).
///
/// - Classified lines are printed as `[CLASS] raw_line`
/// - Collapsed groups are printed as `[... N similar lines: canonical ...]`
pub fn export_plain<W: Write>(
    writer: &mut W,
    items: &[FilteredItem],
    summary: &Summary,
) -> io::Result<()> {
    // Print a short summary header.
    writeln!(writer, "=== Log Summary ===")?;
    writeln!(
        writer,
        "Total: {} lines, Suppressed: {}, Errors: {}, Warnings: {}",
        summary.total_lines, summary.suppressed_lines, summary.error_count, summary.warning_count,
    )?;
    if let (Some(first), Some(last)) = &summary.time_range {
        writeln!(writer, "Time range: {} to {}", first, last)?;
    }
    if !summary.restart_events.is_empty() {
        writeln!(writer, "Restart events: {}", summary.restart_events.len())?;
        for event in &summary.restart_events {
            writeln!(writer, "  {} at {} ({})", event.pod, event.at, event.reason)?;
        }
    }
    writeln!(writer)?;
    writeln!(writer, "=== Filtered Lines ===")?;

    for item in items {
        match item {
            FilteredItem::Line(line) => {
                write_plain_line(writer, line)?;
            }
            FilteredItem::Collapsed(group) => {
                write_plain_collapsed(writer, group)?;
            }
        }
    }

    Ok(())
}

/// Write a single classified line in plain text format.
fn write_plain_line<W: Write>(writer: &mut W, line: &ClassifiedLine) -> io::Result<()> {
    let class_tag = line.class.label().to_uppercase();
    let ts = line.timestamp.map(|t| t.to_string()).unwrap_or_default();

    if ts.is_empty() {
        writeln!(writer, "[{:>11}] {}", class_tag, line.raw)
    } else {
        writeln!(writer, "[{:>11}] {} {}", class_tag, ts, line.raw)
    }
}

/// Write a collapsed group in plain text format.
fn write_plain_collapsed<W: Write>(writer: &mut W, group: &CollapsedGroup) -> io::Result<()> {
    writeln!(
        writer,
        "[... {} similar lines omitted: {} ...]",
        group.count, group.canonical,
    )
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Export pipeline results in the requested format.
///
/// For JSON format, this writes a single [`PipelineOutput`] document.
/// For JSONL and Plain formats, this writes the filtered items directly.
pub fn export<W: Write>(
    writer: &mut W,
    format: OutputFormat,
    output: &PipelineOutput,
    items: &[FilteredItem],
) -> io::Result<()> {
    match format {
        OutputFormat::Json => export_json(writer, output),
        OutputFormat::Jsonl => export_jsonl(writer, items),
        OutputFormat::Plain => export_plain(writer, items, &output.summary),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LineClass;
    use jiff::Timestamp;

    fn make_line(class: LineClass, raw: &str) -> ClassifiedLine {
        ClassifiedLine {
            timestamp: Some(Timestamp::now()),
            pod: "pod-1".to_string(),
            container: Some("app".to_string()),
            class,
            level: Some("ERROR".to_string()),
            msg: Some(raw.to_string()),
            raw: raw.to_string(),
            fields: None,
        }
    }

    fn make_summary() -> Summary {
        Summary {
            time_range: (Some(Timestamp::now()), Some(Timestamp::now())),
            total_lines: 1000,
            suppressed_lines: 900,
            error_count: 10,
            warning_count: 50,
            pods: vec![],
            top_errors: vec![],
            top_warnings: vec![],
            timeline: vec![],
            novel_patterns: vec![],
            restart_events: vec![],
        }
    }

    fn make_output(lines: Vec<ClassifiedLine>) -> PipelineOutput {
        PipelineOutput {
            hint: "Test hint".to_string(),
            summary: make_summary(),
            lines,
        }
    }

    // -- Hint generation ----------------------------------------------------

    #[test]
    fn test_generate_hint() {
        let summary = make_summary();
        let hint = generate_hint("prod-aks", "api", &summary);
        assert!(hint.contains("prod-aks"));
        assert!(hint.contains("api"));
        assert!(hint.contains("900 of 1000 lines omitted"));
        assert!(hint.contains("Troubleshoot mode"));
    }

    // -- JSON export --------------------------------------------------------

    #[test]
    fn test_export_json_produces_valid_json() {
        let line = make_line(LineClass::Error, "ERROR connection refused");
        let output = make_output(vec![line]);

        let mut buf = Vec::new();
        export_json(&mut buf, &output).unwrap();
        let text = String::from_utf8(buf).unwrap();

        // Should be valid JSON.
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(parsed.get("_hint").is_some());
        assert!(parsed.get("summary").is_some());
        assert!(parsed.get("lines").is_some());
    }

    #[test]
    fn test_export_json_compact_is_single_line() {
        let output = make_output(vec![]);
        let mut buf = Vec::new();
        export_json_compact(&mut buf, &output).unwrap();
        let text = String::from_utf8(buf).unwrap();
        // Compact JSON should be a single line (plus trailing newline).
        let line_count = text.trim().lines().count();
        assert_eq!(line_count, 1);
    }

    #[test]
    fn test_export_json_includes_hint() {
        let output = make_output(vec![]);
        let mut buf = Vec::new();
        export_json(&mut buf, &output).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("\"_hint\""));
        assert!(text.contains("Test hint"));
    }

    // -- JSONL export -------------------------------------------------------

    #[test]
    fn test_export_jsonl_one_object_per_line() {
        let items = vec![
            FilteredItem::Line(make_line(LineClass::Error, "err1")),
            FilteredItem::Line(make_line(LineClass::Warning, "warn1")),
            FilteredItem::Collapsed(CollapsedGroup {
                collapsed: true,
                count: 42,
                canonical: "pattern".to_string(),
                sample: "raw sample".to_string(),
            }),
        ];

        let mut buf = Vec::new();
        export_jsonl(&mut buf, &items).unwrap();
        let text = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = text.trim().lines().collect();
        assert_eq!(lines.len(), 3);

        // Each line should be valid JSON.
        for line in &lines {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }
    }

    #[test]
    fn test_export_jsonl_collapsed_has_marker() {
        let items = vec![FilteredItem::Collapsed(CollapsedGroup {
            collapsed: true,
            count: 100,
            canonical: "pat".to_string(),
            sample: "sample".to_string(),
        })];

        let mut buf = Vec::new();
        export_jsonl(&mut buf, &items).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("\"_collapsed\":true"));
        assert!(text.contains("\"count\":100"));
    }

    #[test]
    fn test_export_jsonl_empty_produces_no_output() {
        let mut buf = Vec::new();
        export_jsonl(&mut buf, &[]).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.is_empty());
    }

    // -- Plain text export --------------------------------------------------

    #[test]
    fn test_export_plain_has_summary_header() {
        let items = vec![];
        let summary = make_summary();

        let mut buf = Vec::new();
        export_plain(&mut buf, &items, &summary).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("=== Log Summary ==="));
        assert!(text.contains("Total: 1000 lines"));
        assert!(text.contains("Suppressed: 900"));
        assert!(text.contains("Errors: 10"));
    }

    #[test]
    fn test_export_plain_classified_line_format() {
        let items = vec![FilteredItem::Line(make_line(
            LineClass::Error,
            "ERROR boom",
        ))];
        let summary = make_summary();

        let mut buf = Vec::new();
        export_plain(&mut buf, &items, &summary).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("[      ERROR]"));
        assert!(text.contains("ERROR boom"));
    }

    #[test]
    fn test_export_plain_collapsed_format() {
        let items = vec![FilteredItem::Collapsed(CollapsedGroup {
            collapsed: true,
            count: 847,
            canonical: "INFO GET /api/users".to_string(),
            sample: "sample".to_string(),
        })];
        let summary = make_summary();

        let mut buf = Vec::new();
        export_plain(&mut buf, &items, &summary).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("[... 847 similar lines omitted"));
        assert!(text.contains("INFO GET /api/users"));
    }

    // -- Dispatch -----------------------------------------------------------

    #[test]
    fn test_dispatch_json() {
        let output = make_output(vec![]);
        let mut buf = Vec::new();
        export(&mut buf, OutputFormat::Json, &output, &[]).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("\"_hint\""));
    }

    #[test]
    fn test_dispatch_jsonl() {
        let items = vec![FilteredItem::Line(make_line(LineClass::Error, "err"))];
        let output = make_output(vec![]);
        let mut buf = Vec::new();
        export(&mut buf, OutputFormat::Jsonl, &output, &items).unwrap();
        let text = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = text.trim().lines().collect();
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_dispatch_plain() {
        let output = make_output(vec![]);
        let mut buf = Vec::new();
        export(&mut buf, OutputFormat::Plain, &output, &[]).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("=== Log Summary ==="));
    }

    // -- Plain text with no timestamps -------------------------------------

    #[test]
    fn test_plain_line_without_timestamp() {
        let mut line = make_line(LineClass::Novel, "new pattern observed");
        line.timestamp = None;

        let mut buf = Vec::new();
        write_plain_line(&mut buf, &line).unwrap();
        let text = String::from_utf8(buf).unwrap();
        // Should still format without a timestamp.
        assert!(text.contains("[      NOVEL]"));
        assert!(text.contains("new pattern observed"));
        // Should NOT have a double space where timestamp would be.
        assert!(!text.contains("[      NOVEL]  "));
    }

    // -- Plain text with restart events ------------------------------------

    #[test]
    fn test_plain_shows_restart_events() {
        use crate::types::RestartEvent;

        let summary = Summary {
            restart_events: vec![RestartEvent {
                pod: "api-pod".to_string(),
                at: Timestamp::now(),
                reason: "OOMKilled".to_string(),
            }],
            ..make_summary()
        };

        let mut buf = Vec::new();
        export_plain(&mut buf, &[], &summary).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("Restart events: 1"));
        assert!(text.contains("api-pod"));
        assert!(text.contains("OOMKilled"));
    }
}
