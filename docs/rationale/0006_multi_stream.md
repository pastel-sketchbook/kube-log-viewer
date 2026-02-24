# ADR-0006: Multi-Stream Log Viewing

**Date:** 2026-02-24
**Status:** Accepted

## Context

Debugging microservices often requires correlating logs from multiple pods simultaneously. The current single-stream design forces users to switch between pods, losing temporal context. Two complementary viewing modes address different workflows:

1. **Merged view** — all streams interleaved chronologically in one pane, each line prefixed with a color-coded `[pod-name]` tag. Best for tracing request flows across services.
2. **Split view** — two horizontal panes (top/bottom), each showing one stream independently. Best for comparing behavior between two specific pods (e.g., old vs new replica).

Vertical split was rejected because log lines are already wide (timestamps + JSON flattening); halving width makes them unreadable.

## Decision

### Data Model

- **`TaggedLine`** — Each log line carries its source pod name. System messages (errors, az-login) use an empty source string.
- **`StreamMode`** — Enum: `Single` (default, current behavior) | `Merged` | `Split`.
- **`LogStreamHandle`** — Holds pod name, container, and a `watch::Sender<bool>` for cancellation. Replaces the single `log_cancel_tx`.
- **`streams: Vec<LogStreamHandle>`** — Supports up to 4 concurrent streams. Each stream sends `AppEvent::LogLine(source, line)` via the shared `mpsc` channel.

### Event Changes

`AppEvent::LogLine(String)` becomes `AppEvent::LogLine(String, String)` — `(source, line)`. The log streaming function receives the pod name and tags every line it sends.

### Keybindings

| Key | Action |
|-----|--------|
| `M` | Add currently selected pod as additional stream (enters Merged mode) |
| `V` | Cycle view: Merged → Split → Single (only when multiple streams active) |
| `X` | Remove most recently added stream; return to Single when one remains |

`Enter` on a pod always controls the **primary stream** (slot 0) and clears all others, returning to Single mode. This preserves backward compatibility.

### Rendering

- **Merged:** Single log pane. Each line rendered with a `[short-pod-name]` prefix span in the stream's assigned color, followed by the normal colorized log content.
- **Split:** Log area divided into two equal `Rect` regions (top/bottom). Each pane renders independently with its own scroll offset, follow mode, and border title showing the pod name. Focus switches between panes via number keys `1`/`2`.
- **Color assignment:** A fixed 6-color palette (`Cyan, Yellow, Magenta, Green, Red, Blue`). Stream index modulo palette length determines color.

### Scroll State

- **Single/Merged:** One `log_scroll_offset` and `follow_mode` (existing fields).
- **Split:** Each pane has independent `scroll_offset` and `follow_mode` stored in the `LogStreamHandle`. The active pane is tracked by `active_pane: usize`. Scroll keys (j/k/g/G/PgUp/PgDn) and follow toggle (`f`) apply to the active pane only.

### Filtering

`filtered_log_lines()` returns `Vec<&TaggedLine>`. For merged mode, all lines pass through. For split mode, the UI filters by `source` when rendering each pane. Search and time range filters apply to all lines regardless of source.

## Consequences

- `Vec<String>` → `Vec<TaggedLine>` touches many call sites (pushes, tests, filtered iteration).
- `AppEvent::LogLine` gains a source field — `k8s/logs.rs` must receive and forward the pod name.
- Split mode adds per-pane scroll state, increasing `App` complexity.
- Maximum 4 streams prevents resource exhaustion from too many concurrent K8s API connections.
- Lines arrive in approximate timestamp order from K8s; no explicit sorting needed for merged view.
