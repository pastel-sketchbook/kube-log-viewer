# ADR-0001: Application Design

**Date:** 2026-02-24
**Status:** Accepted

## Context

Kubernetes operators and developers frequently need to inspect pod logs for debugging and monitoring. Existing tools each have tradeoffs:

| Tool | Limitation |
|------|-----------|
| `kubectl logs` | Single pod, no TUI, no live filtering, requires re-running for each pod |
| Stern | Multi-pod streaming but no interactive navigation, no TUI |
| k9s | Full cluster management TUI -- heavyweight for "just reading logs" |
| Lens | GUI, Electron-based, resource-heavy |

**Goal:** A focused, fast, keyboard-driven TUI that does one thing well -- stream and search Kubernetes pod logs interactively.

## Decision: Crate Selection

| Concern | Choice | Alternatives Considered | Rationale |
|---------|--------|------------------------|-----------|
| K8s client | `kube-rs` | Shelling out to `kubectl` | Native async Rust, proper error types, `Api::log_stream` for live streaming, no external binary dependency |
| TUI framework | `ratatui` | `cursive`, raw `crossterm` | Active community, composable widget model, immediate-mode `Frame`-based rendering eliminates flicker |
| Terminal backend | `crossterm` | `termion`, `termwiz` | Cross-platform (Windows + macOS + Linux), async `EventStream`, first-class ratatui backend |
| Async runtime | `tokio` | `async-std` | Required by `kube-rs`, dominant ecosystem, `select!` macro for multiplexing event sources |
| Error handling | `anyhow` | `thiserror`, `Box<dyn Error>` | Application-level binary; `.context()` for actionable error messages without boilerplate |
| Regex | `regex` | `fancy-regex`, substring only | Good balance of features and compile-time; start with substring filter, upgrade path to full regex |

## Decision: Architecture & Event Model

The application follows a **channel-based async architecture** with clear separation between I/O and rendering:

```
┌──────────────┐     ┌──────────────────┐     ┌────────────┐
│  crossterm   │     │   tokio::spawn   │     │   Main     │
│  EventStream │────>│   K8s tasks      │────>│   Loop     │
│  (terminal)  │     │   (pods, logs,   │     │            │
└──────────────┘     │    namespaces)   │     │  select! { │
                     └──────────────────┘     │    term,   │
                            │                 │    k8s_rx, │
                            │ mpsc channel    │    tick    │
                            └────────────────>│  }         │
                                              │            │
                                              │  render()  │
                                              └────────────┘
```

**Components:**
- **Main thread** -- Runs the render loop and event dispatch via `tokio::select!`
- **Background tasks** -- K8s API calls (`list_pods`, `list_namespaces`, `log_stream`) via `tokio::spawn`
- **App -> Background** -- Log stream cancellation via `tokio::sync::watch` channel (cooperative)
- **Background -> App** -- Results via `mpsc::unbounded_channel<AppEvent>`
- **Terminal -> App** -- `crossterm::event::EventStream` integrated directly into `select!`

**Why this model:**
- The TUI stays responsive even when K8s API calls are slow or the cluster is unreachable.
- Unbounded channel is acceptable: log lines are capped at 50,000 entries, and K8s metadata events are infrequent.
- `watch` channel for cancellation is efficient -- `changed()` is cancel-safe and doesn't allocate.

## Decision: K8s Client Management

- One `kube::Client` per active context.
- Client is recreated when the user switches contexts via `Config::from_kubeconfig(&KubeConfigOptions { context: Some(name) })`.
- `Client` is `Clone`-cheap (wraps `Arc` internally), so clones are passed into spawned tasks.
- Context list is read from `~/.kube/config` via `kube::config::Kubeconfig::read()` (synchronous file read, acceptable at startup and on context switch).
- Namespace list fetched via `Api::<Namespace>::all(client).list()`.
- Pod list fetched via `Api::<Pod>::namespaced(client, ns).list()`.
- Log streaming via `Api::<Pod>::log_stream(name, &LogParams { follow: true, tail_lines: 100, .. })`.

## Decision: UI Layout

```
┌─ Header ─────────────────────────────────────────────────┐
│ ctx: minikube  │  ns: default  │  ? help                 │
├─ Pods (5) ─────┬─ Logs: my-pod / main ───────────────────┤
│ ● my-pod-1     │ 2026-02-24 10:00:00 Starting up...      │
│   my-pod-2     │ 2026-02-24 10:00:01 Listening on :8080  │
│ ● my-pod-3     │ 2026-02-24 10:00:02 Request received    │
│   my-pod-4     │ 2026-02-24 10:00:03 Processing...       │
│                │                                         │
├────────────────┴─────────────────────────────────────────┤
│ ↑↓ nav │ Enter select │ / search │ n ns │ c ctx │ q quit │
└──────────────────────────────────────────────────────────┘
```

- **25/75 horizontal split** -- Pod list (left), log viewer (right)
- **Header (3 rows)** -- Active context, namespace, help hint
- **Status bar (1 row)** -- Context-sensitive keybinding hints (changes in search mode)
- **Popups** -- Namespace, context, and container pickers rendered as centered overlays with `Clear` widget underneath

## Decision: Keybinding Philosophy

Vim-inspired navigation with mnemonic single-letter actions:

| Category | Keys | Notes |
|----------|------|-------|
| Navigation | `j`/`k`, `↑`/`↓` | Consistent with vim and most TUIs |
| Selection | `Enter` | Select pod, start streaming |
| Focus | `Tab` | Toggle between Pods and Logs panels |
| Scroll | `g`/`G`, `PgUp`/`PgDn` | Top/bottom, page scroll |
| Actions | `n`, `c`, `s`, `f`, `w` | Namespace, context, container (s=select), follow, wrap |
| Search | `/` | Enter search mode (vim convention) |
| Help | `?` | Toggle help overlay |
| Quit | `q`, `Ctrl+C` | Normal quit, force quit |

No modifier keys beyond `Ctrl+C`. Single keystrokes for all actions keep the interface fast.

## Decision: Log Line Management

- **Storage:** `Vec<String>` -- simple, cache-friendly for sequential access.
- **Cap:** 50,000 lines max. When exceeded, drain the oldest 10,000 lines. This prevents unbounded memory growth during long streaming sessions.
- **Filtering:** Case-insensitive substring match on demand. Not pre-computed -- `filtered_log_lines()` scans on each render. Acceptable because:
  - 50k string comparisons per frame is fast (~1ms)
  - Avoids maintaining a parallel filtered index that must stay in sync
- **Follow mode:** When active, `log_scroll_offset` is pinned to the bottom on each new line.
- **Colorization:** Log level keywords (`ERROR`, `WARN`, `INFO`, `DEBUG`) are detected via substring match and rendered in red, yellow, white, and gray respectively.
- **Search highlight:** Matching substrings within visible lines are rendered with inverted colors (black on yellow).

## Future Considerations

Explicitly deferred to keep the initial implementation focused:

- **Regex search** -- Start with substring, add regex toggle later
- **Pod watch / auto-refresh** -- Start with manual list; add `kube::runtime::watcher` later
- **Multiple log panes** -- Single log view initially
- **Log export to file** -- Not in v0.1
- **Custom color themes** -- Hardcoded palette initially
- **Timestamp parsing** -- Display raw log lines; structured parsing later
- **Resource metrics** -- CPU/memory display is out of scope (this is a log viewer)
