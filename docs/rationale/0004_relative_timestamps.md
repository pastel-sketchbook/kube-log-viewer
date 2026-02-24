# ADR-0004: Timestamp Display Modes and Time Range Filtering

**Date:** 2026-02-24
**Status:** Accepted

## Context

Log lines contain UTC timestamps like `2026-02-24T16:36:51.600Z`. Two usability gaps:

1. **Timezone mismatch.** Operators think in local time, but K8s logs are UTC. Mental conversion is error-prone.
2. **No time-based filtering.** When debugging an incident, users need to narrow logs to a specific window (e.g. "last 5 minutes") without scrolling through thousands of lines.

## Decision 1: Timestamp Display Modes

`T` (shift-T) cycles through three display modes:

| Mode     | Display                     | Example                  |
|----------|-----------------------------|--------------------------|
| Utc      | Original timestamp as-is    | `2026-02-24T16:36:51.600Z` |
| Local    | Converted to local timezone | `2026-02-24 10:36:51`    |
| Relative | Time since now              | `3s ago`                 |

Default: **Local** (most intuitive for operators).

**Relative format:**

| Delta    | Format     | Example  |
|----------|------------|----------|
| < 60s    | `{n}s ago` | `3s ago` |
| < 60m    | `{n}m ago` | `5m ago` |
| < 24h    | `{n}h ago` | `2h ago` |
| >= 24h   | `{n}d ago` | `1d ago` |

### Integration Point

Timestamp conversion happens **inside** `colorize_log_line()` at the `TIMESTAMP_RE` match site. The matched text is parsed, converted according to the current mode, and rendered in `theme.muted` color. Parse failures silently keep the original text.

```
log_lines[i]
      │
      ▼
format_json_line()          ← JSON flattening (when json_mode=true)
      │
      ▼
colorize_log_line()         ← TIMESTAMP_RE match:
      │                         Utc: keep original (muted)
      │                         Local: parse → local format (muted)
      │                         Relative: parse → "3s ago" (muted)
      │                       level keyword detection → level color
      ▼
highlight_search()          ← (if search query active, replaces colorize step)
      │
      ▼
rendered Span/Line
```

### Timestamp Parsing

- `chrono::DateTime::parse_from_rfc3339()` for RFC 3339 (`2026-02-24T16:36:51.600Z`, `...+05:30`)
- `NaiveDateTime::parse_from_str()` with `%Y-%m-%d %H:%M:%S` fallback (assumed UTC)
- Parse failures keep original text unchanged

## Decision 2: Time Range Filtering

A time range filter narrows displayed log lines to a specific window. Triggered by `R` (shift-R) which opens a popup with predefined ranges and a custom option.

**Predefined ranges:**

| Label        | Meaning            |
|--------------|--------------------|
| All          | No filter (default)|
| Last 5m      | Now minus 5 min    |
| Last 15m     | Now minus 15 min   |
| Last 30m     | Now minus 30 min   |
| Last 1h      | Now minus 1 hour   |
| Last 6h      | Now minus 6 hours  |
| Last 24h     | Now minus 24 hours |

**Custom range:** User types a relative duration like `5m`, `1h30m`, `2h` which is parsed and applied as "now minus duration".

### Filtering Mechanism

Time range filtering is applied in `App::filtered_log_lines()` alongside the existing search filter:

1. For each log line, attempt to parse the leading timestamp (or JSON `time` field if `json_mode` is on).
2. If a `time_range` is set and the timestamp falls outside the range, exclude the line.
3. Lines without parseable timestamps are always included (they may be continuation lines or error messages).

### State

```rust
pub enum TimeRange {
    All,
    Last(Duration),
}
```

The active range is stored as `App::time_range: TimeRange`. The popup sets this value.

## Consequences

- Local time is the default display — operators see familiar times without mental UTC conversion.
- Time range filter reduces noise during incident investigation.
- Both features reuse `parse_log_timestamp()` — single parsing function, two consumers.
- `chrono` already in `Cargo.toml` — no new dependencies.
- Filtering by time adds a per-line timestamp parse in `filtered_log_lines()`, but this is bounded by `log_lines.len()` (capped at 50,000) and the parse is fast (regex check + chrono parse only if regex matches).
