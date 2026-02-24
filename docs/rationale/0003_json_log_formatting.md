# ADR-0003: Unified JSON Log Formatting

**Date:** 2026-02-24
**Status:** Accepted

## Context

Many Kubernetes workloads emit structured JSON logs. In the TUI, these appear as dense single-line JSON objects that are difficult to scan visually:

```
{"time":"2026-02-24T16:36:51.600Z","id":"6e9d31a8...","remote_ip":"10.150.37.65","host":"ge-mayday-metrics.azdev.americannational.com","method":"GET","uri":"/metrics","status":200,"error":"","latency_human":"875.124us","bytes_in":0,"bytes_out":0}
```

This is unreadable at a glance, especially for high-throughput HTTP access logs where every line is a JSON blob. Users need to mentally parse braces, quotes, and commas to find the relevant fields.

## Decision: Render-Time JSON Flattening

Add a `json_mode: bool` toggle (default: **on**) to flatten JSON log lines into a human-readable `key=value` format at render time. Toggle with `J` (shift-J).

**Formatted output example:**

```
2026-02-24T16:36:51.600Z GET /metrics status=200 latency_human=875.124us remote_ip=10.150.37.65 host=ge-mayday-metrics.azdev.americannational.com bytes_in=0 bytes_out=0
```

### Well-Known Field Extraction

Priority fields are extracted and placed first in a natural reading order. Remaining fields follow as `key=value` pairs.

| Priority | Field names (checked in order)                          | Format          |
|----------|---------------------------------------------------------|-----------------|
| 1        | `time`, `timestamp`, `ts`, `@timestamp`, `datetime`    | Raw value       |
| 2        | `level`, `severity`, `loglevel`, `log_level`, `lvl`     | `[LEVEL]`       |
| 3        | `msg`, `message`                                        | Raw value       |
| 4        | All remaining fields                                    | `key=value`     |

**Skipped values:** empty strings and `null` are omitted from the remaining fields to reduce noise.

### Why Render-Time, Not Ingestion-Time

- **Raw data preserved.** `log_lines: Vec<String>` stores original JSON. Toggling `J` shows raw output instantly -- no data loss.
- **Search operates on raw text.** The `/` search filter matches against original log lines, so users can search for JSON field names like `"remote_ip"` even in formatted mode.
- **Bounded cost.** Only visible lines are formatted (~terminal height, typically 30-50 lines). Parsing 50 JSON objects per frame is negligible.

### Render Pipeline

```
log_lines[i]  (raw JSON string)
      │
      ▼
format_json_line()          ← JSON parsing + flattening (only when json_mode=true)
      │                       returns Cow::Owned("2026-02-24T16:36:51.600Z GET /metrics status=200 ...")
      │                       non-JSON lines pass through as Cow::Borrowed
      ▼
colorize_log_line()         ← TIMESTAMP_RE strips leading timestamp → muted color
      │                       level keyword detection (ERROR/WARN/DEBUG) → level color
      ▼
highlight_search()          ← (if search query active, replaces colorize step)
      │
      ▼
zebra striping              ← odd-row background
      │
      ▼
rendered Span/Line
```

The critical ordering: **parsed JSON first, colorization second.** This ensures the extracted `time` field (now a leading timestamp in the flattened string) is matched by `TIMESTAMP_RE` and rendered in `theme.muted` color. The extracted `level` field (formatted as `[ERROR]`/`[WARN]`/`[DEBUG]`) is caught by the existing keyword detection and colored accordingly. No new color logic or theme fields required.

Non-JSON lines pass through unchanged (`Cow::Borrowed`).

## Implementation

- **Dependency:** `serde_json` for `Value` parsing.
- **App state:** `json_mode: bool`, default `true`.
- **Key binding:** `J` (shift-J) toggles `json_mode`.
- **Render path:** In `ui/logs.rs`, visible lines are mapped through `format_json_line()` (returning `Cow<str>`) before colorization.
- **Statusbar:** `J` hint added with accent-color highlight when active (consistent with `f`/`w` toggle pattern).
- **Help overlay:** `J` documented under Actions.

## Consequences

- JSON logs become scannable at a glance without leaving the TUI.
- The feature is format-agnostic -- any valid JSON object line is flattened, regardless of schema.
- Users working with raw JSON can toggle `J` to see the original output.
- `serde_json` adds a compile-time dependency but no runtime cost for non-JSON lines (early `starts_with('{')` check).
