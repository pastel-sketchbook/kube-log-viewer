# TODO -- kube-log-viewer

Implementation roadmap organized in phases. Each phase builds on the previous.

## Phase 1: Project Scaffold

- [ ] Initialize Cargo project (`Cargo.toml`, `src/main.rs`)
- [ ] Create directory structure (`src/k8s/`, `src/ui/`, `docs/rationale/`)
- [ ] Configure dependencies (ratatui, kube-rs, tokio, crossterm, anyhow, regex, futures, k8s-openapi)
- [ ] Create `.gitignore` for Rust
- [ ] Update `AGENTS.md` to reflect kube-log-viewer

## Phase 2: Core Infrastructure

- [ ] Implement `AppEvent` enum (`src/event.rs`)
- [ ] Implement `App` struct with all state fields (`src/app.rs`)
- [ ] Implement main event loop with `tokio::select!` over terminal events, K8s channel, and tick (`src/app.rs`)
- [ ] Implement terminal setup/teardown with proper restoration on panic (`src/main.rs`)
- [ ] Implement key handling dispatch: normal mode, search mode, popup mode (`src/app.rs`)

## Phase 3: Kubernetes Layer

- [ ] Implement client creation with context support (`src/k8s/mod.rs`)
- [ ] Implement context listing from kubeconfig (`src/k8s/contexts.rs`)
- [ ] Implement namespace listing via K8s API (`src/k8s/namespaces.rs`)
- [ ] Implement pod listing with status, ready count, restart count, container names (`src/k8s/pods.rs`)
- [ ] Implement log streaming with `follow: true` and cooperative cancellation via `watch` channel (`src/k8s/logs.rs`)

## Phase 4: UI Layer

- [ ] Implement main layout: header / body (25/75 split) / statusbar (`src/ui/mod.rs`)
- [ ] Implement header bar with context, namespace, help hint (`src/ui/header.rs`)
- [ ] Implement pod list panel with status icons and highlight (`src/ui/pods.rs`)
- [ ] Implement log viewer panel with scroll offset calculation (`src/ui/logs.rs`)
- [ ] Implement popup overlay for namespace, context, and container pickers (`src/ui/popup.rs`)
- [ ] Implement status bar with context-sensitive keybinding hints (`src/ui/statusbar.rs`)
- [ ] Implement help overlay with full keybinding reference (`src/ui/mod.rs`)

## Phase 5: Feature Completion

- [ ] Log search/filter with case-insensitive substring matching
- [ ] Search result highlighting (black on yellow for matches)
- [ ] Follow mode: auto-scroll to bottom on new log lines
- [ ] Line wrap toggle
- [ ] Multi-container support: container picker popup, per-container log streaming
- [ ] Log level colorization (ERROR=red, WARN=yellow, INFO=white, DEBUG=gray)
- [ ] Log line cap at 50,000 with oldest-drain strategy
- [ ] Graceful error display (K8s errors shown in log panel, not crash)

## Phase 6: Polish & Testing

- [ ] Unit tests: `PodInfo` extraction from K8s API objects
- [ ] Unit tests: log filtering and search highlight logic
- [ ] Unit tests: key handling state transitions
- [ ] Integration test: full event loop with mock K8s responses
- [ ] `cargo clippy -- -D warnings` passes clean
- [ ] `cargo test` passes clean
- [ ] Error handling audit: no `unwrap()` in non-test code
- [ ] Terminal restoration audit: ensure cleanup on panic (catch_unwind or panic hook)
- [ ] README: add screenshot / demo GIF

## Future (Post v0.1)

- [ ] Regex search mode (toggle between substring and regex)
- [ ] Pod auto-refresh via `kube::runtime::watcher`
- [ ] Log export to file
- [ ] Custom color themes / config file
- [ ] Timestamp parsing and relative time display
- [ ] Multiple simultaneous log streams (split pane)
