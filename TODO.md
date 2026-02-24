# TODO -- kube-log-viewer

Implementation roadmap organized in phases. Each phase builds on the previous.

## Phase 1: Project Scaffold

- [x] Initialize Cargo project (`Cargo.toml`, `src/main.rs`)
- [x] Create directory structure (`src/k8s/`, `src/ui/`, `docs/rationale/`)
- [x] Configure dependencies (ratatui, kube-rs, tokio, crossterm, anyhow, regex, futures, k8s-openapi)
- [x] Create `.gitignore` for Rust
- [x] Update `AGENTS.md` to reflect kube-log-viewer

## Phase 2: Core Infrastructure

- [x] Implement `AppEvent` enum (`src/event.rs`)
- [x] Implement `App` struct with all state fields (`src/app.rs`)
- [x] Implement main event loop with `tokio::select!` over terminal events, K8s channel, and tick (`src/app.rs`)
- [x] Implement terminal setup/teardown with proper restoration on panic (`src/main.rs`)
- [x] Implement key handling dispatch: normal mode, search mode, popup mode (`src/app.rs`)

## Phase 3: Kubernetes Layer

- [x] Implement client creation with context support (`src/k8s/mod.rs`)
- [x] Implement context listing from kubeconfig (`src/k8s/contexts.rs`)
- [x] Implement namespace listing via K8s API (`src/k8s/namespaces.rs`)
- [x] Implement pod listing with status, ready count, restart count, container names (`src/k8s/pods.rs`)
- [x] Implement log streaming with `follow: true` and cooperative cancellation via `watch` channel (`src/k8s/logs.rs`)

## Phase 4: UI Layer

- [x] Implement main layout: header / body (25/75 split) / statusbar (`src/ui/mod.rs`)
- [x] Implement header bar with context, namespace, help hint (`src/ui/header.rs`)
- [x] Implement pod list panel with status icons and highlight (`src/ui/pods.rs`)
- [x] Implement log viewer panel with scroll offset calculation (`src/ui/logs.rs`)
- [x] Implement popup overlay for namespace, context, and container pickers (`src/ui/popup.rs`)
- [x] Implement status bar with context-sensitive keybinding hints (`src/ui/statusbar.rs`)
- [x] Implement help overlay with full keybinding reference (`src/ui/mod.rs`)

## Phase 5: Feature Completion

- [x] Log search/filter with case-insensitive substring matching
- [x] Search result highlighting (black on yellow for matches)
- [x] Follow mode: auto-scroll to bottom on new log lines
- [x] Line wrap toggle
- [x] Multi-container support: container picker popup, per-container log streaming
- [x] Log level colorization (ERROR=red, WARN=yellow, INFO=white, DEBUG=gray)
- [x] Log line cap at 50,000 with oldest-drain strategy
- [x] Graceful error display (K8s errors shown in log panel, not crash)
- [x] Structured tracing: daily-rotated log file via `tracing` + `tracing-appender`
- [x] `lib.rs` for crate-level re-exports (integration test access)

## Phase 6: Polish & Testing

- [x] Unit tests: `PodInfo` extraction from K8s API objects
- [x] Unit tests: log filtering and search highlight logic
- [x] Unit tests: key handling state transitions
- [x] Integration test: full event loop with mock K8s responses (12 tests)
- [x] `cargo clippy -- -D warnings` passes clean
- [x] `cargo test` passes clean
- [x] Error handling audit: no `unwrap()` in non-test code
- [x] Terminal restoration audit: ensure cleanup on panic (catch_unwind or panic hook)

## Future (Post v0.1)

- [x] Inline error display with `[ERROR]` prefix in log lines
- [x] Automatic `az login` on Azure auth errors
- [x] Theme system with 16 palettes and `t` cycle key
- [x] Wide log view toggle (`w`)
- [x] Zebra striping on log lines
- [x] Timestamp detection and muted color rendering
- [x] JSON log flattening with `J` toggle (default on)
- [x] Toggle status indication via accent-highlighted statusbar labels
- [x] CI/CD: GitHub Actions for clippy/test/build (macOS + Ubuntu)
- [x] CI/CD: Multi-arch release workflow (4 targets, GitHub Release)
- [ ] README: add screenshot / demo GIF
- [ ] Regex search mode (toggle between substring and regex)
- [x] Pod auto-refresh via `kube::runtime::watcher`
- [ ] Log export to file
- [ ] Config file for themes / keybindings
- [x] Timestamp display modes: UTC / Local / Relative (`T` cycle)
- [x] Time range filtering popup (`R`) with predefined ranges
- [ ] Multiple simultaneous log streams (split pane)
