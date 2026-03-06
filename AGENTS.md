# ROLES AND EXPERTISE

This codebase operates with two distinct but complementary roles:

## Implementor Role

You are a senior Rust engineer building a high-performance TUI application and a pipe-friendly CLI for LLM integration. You implement changes with attention to error handling, async correctness, and terminal UX.

**Responsibilities:**
- Write idiomatic Rust with proper error handling (`anyhow` + `.context()`)
- Design clean module boundaries between K8s layer, app state, and UI rendering
- Follow TDD principles: write tests alongside implementation
- Ensure async operations don't block the render loop
- Handle terminal setup/teardown robustly (including panic recovery)

## Reviewer Role

You are a senior engineer who evaluates changes for quality, correctness, and adherence to Rust + TUI best practices.

**Responsibilities:**
- Verify error handling is comprehensive (no `unwrap()` in non-test code; `.expect()` only with safety comment)
- Check that async code doesn't have subtle race conditions or deadlocks
- Ensure TUI rendering doesn't flicker or corrupt terminal state
- Validate K8s client lifecycle (proper cancellation, no leaked tasks)
- Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`

# SCOPE OF THIS REPOSITORY

This repository contains `kube-log-viewer`, a Rust workspace with two binaries sharing a core library:

- **`kube-log-viewer`** (TUI) — interactive terminal UI for streaming and searching Kubernetes pod logs
- **`kube-log`** (CLI) — pipe-friendly CLI for LLM integration with classify-filter-reduce pipeline and JSON output

Both binaries:
- **Connect** to Kubernetes clusters via `~/.kube/config`
- **List** namespaces and pods with status indicators
- **Stream** pod logs in real-time with follow mode
- **Filter** log lines by keyword search
- **Support** multi-container pods with container selection
- **Switch** between Kubernetes contexts

The CLI additionally:
- **Classifies** log lines (error, warning, lifecycle, health check, repeated, novel, normal)
- **Filters** by suppressing noise (health checks, repeated lines) in summary mode
- **Reduces** to bounded summaries (error buckets, timeline, restart events)
- **Exports** as JSON, JSONL, or plain text for piping into LLM tools

**Runtime requirements:**
- Any OS with Rust toolchain (1.75+)
- Access to a Kubernetes cluster (valid kubeconfig)

# ARCHITECTURE

```
kube-log-viewer/                            (workspace root)
├── Cargo.toml                              # Workspace definition
├── crates/
│   ├── kube-log-core/                      # Shared library crate
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                      # Crate re-exports: k8s, parse, classify, filter, reduce, export, types
│   │       ├── parse.rs                    # Timestamp regex, JSON flattening
│   │       ├── types.rs                    # Pipeline types: LineClass, ClassifiedLine, Summary, FilterConfig, etc.
│   │       ├── classify.rs                 # Log line classifier with structural dedup (seen-set)
│   │       ├── filter.rs                   # Noise suppression, collapse groups, class filtering
│   │       ├── reduce.rs                   # Bounded summary: error buckets, timeline, restart events
│   │       ├── export.rs                   # JSON/JSONL/plain formatters, hint generation
│   │       └── k8s/
│   │           ├── mod.rs                  # Client creation (kubeconfig-based)
│   │           ├── contexts.rs             # Context listing
│   │           ├── namespaces.rs           # Namespace listing
│   │           ├── pods.rs                 # Pod listing + PodInfo + watch stream
│   │           └── logs.rs                 # Log streaming with cancellation
│   ├── kube-log-tui/                       # TUI binary crate
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── main.rs                     # Entry point, terminal setup/teardown
│   │   │   ├── lib.rs                      # Crate re-exports: app, event, prefs, ui
│   │   │   ├── app.rs                      # App state, event loop, key handling
│   │   │   ├── event.rs                    # AppEvent enum
│   │   │   ├── prefs.rs                    # User preferences (theme persistence)
│   │   │   └── ui/
│   │   │       ├── mod.rs                  # Main layout, help overlay
│   │   │       ├── header.rs               # Header bar
│   │   │       ├── pods.rs                 # Pod list panel
│   │   │       ├── logs.rs                 # Log viewer panel
│   │   │       ├── popup.rs                # Popup overlays (pickers)
│   │   │       ├── statusbar.rs            # Keybinding hints
│   │   │       └── theme.rs                # Theme system (16 palettes)
│   │   └── tests/
│   │       └── integration.rs              # Integration tests
│   └── kube-log-cli/                       # CLI binary crate (pipe-friendly, LLM-oriented)
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs                     # Entry point, shell completion env, subcommand dispatch
│           ├── cli.rs                      # Clap derive: Cli, Command enum, LogsArgs, PodsArgs, etc.
│           ├── output.rs                   # Pipeline wire-up: run_logs (batch/follow), run_pods, run_contexts, run_namespaces
│           └── complete.rs                 # Dynamic shell completers: context, namespace, pod
├── docs/rationale/                         # Architecture decision records
│   ├── 0001_application_design.md          # Core design rationale
│   ├── 0002_error_handling_and_auth.md     # Error handling and authentication design
│   ├── 0003_json_log_formatting.md         # JSON log formatting design
│   ├── 0004_relative_timestamps.md         # Relative timestamp design
│   ├── 0005_pod_auto_refresh.md            # Pod auto-refresh design
│   ├── 0006_multi_stream.md                # Multi-stream design
│   └── 0007_llm_cli_dual_interface.md      # CLI + LLM integration design (664 lines, the blueprint)
├── TODO.md                                 # Phased implementation roadmap
└── .editorconfig                           # Editor settings
```

**Local reference files (gitignored):**
- `.skills/` — API reference and cheatsheets for dependencies (e.g., `kube.md`). Read these before looking up crate APIs in cargo registry source.

**Data flow (TUI):**
1. `main.rs` sets up the terminal and launches the async event loop
2. `App::run()` uses `tokio::select!` to multiplex terminal events, K8s channel events, and ticks
3. K8s operations run in `tokio::spawn` tasks, sending results via `mpsc::unbounded_channel<AppEvent>`
4. On each loop iteration, `ui::render()` draws the current state to the terminal via ratatui
5. Log streaming uses `Api::log_stream` with cooperative cancellation via `watch` channel

**Data flow (CLI):**
1. `main.rs` parses args via clap, checks for shell completion env, then dispatches to `output.rs` functions
2. `run_logs()` (batch): fetch logs → classify each line → filter (suppress noise) → reduce (bounded summary) → export (JSON/JSONL/plain) → stdout
3. `run_logs_follow()` (streaming): classify each line as it arrives → write JSONL to stdout → ctrl-c to stop
4. `run_pods()`/`run_contexts()`/`run_namespaces()`: query K8s API → format as JSON → stdout
5. All errors are written as structured JSON to stderr for machine consumption

# CORE DEVELOPMENT PRINCIPLES

- **No Panics**: Never use `unwrap()` in non-test code. Use `?` with `anyhow::Context`. `.expect()` is permitted only when the invariant is logically guaranteed, with a safety comment explaining why.
- **Error Messages**: Provide actionable error messages with context. K8s connection errors should suggest checking kubeconfig.
- **Non-blocking Render**: Never perform blocking I/O in the render path. All K8s calls go through background tasks.
- **Graceful Degradation**: If a K8s call fails, show the error in the UI rather than crashing. The TUI must remain interactive.
- **Terminal Safety**: Always restore terminal state on exit, including on panic. Use a panic hook or `catch_unwind` wrapper.
- **Testing**: Unit tests for state transitions, pod info extraction, log filtering. Integration tests for event handling.
- **Pre-commit Gate**: Always run `task check:all && task install` before committing. Only commit if both pass.
- **No Pushing Without Permission**: Never `git push` or `git push --tags` unless the user explicitly asks. Tags trigger CI releases, so avoid unnecessary pushes.

# COMMIT CONVENTIONS

Use the following prefixes:
- `feat`: New feature or capability
- `fix`: Bug fix
- `refactor`: Code improvement without behavior change
- `test`: Adding or improving tests
- `docs`: Documentation changes
- `chore`: Tooling, dependencies, configuration

# RUST-SPECIFIC GUIDELINES

## Error Handling
- Use `anyhow::Result` for all fallible functions
- Always add `.context()` or `.with_context()` for actionable error messages
- Return `Result` from all public functions
- K8s errors should include the resource name (pod, namespace) in context

## Async & Concurrency
- Use `tokio` as the sole async runtime
- Use `mpsc::unbounded_channel` for background task -> main communication
- Use `watch` channel for cooperative cancellation of log streams
- Never `.await` inside a `terminal.draw()` closure
- Spawned tasks must not hold references to `App` -- clone needed data before spawning

## TUI Rendering
- Use `ratatui` immediate-mode rendering via `terminal.draw(|f| ...)`
- All rendering logic lives in `src/ui/` modules
- UI functions take `&Frame` (or `&mut Frame`) and `&App` (or `&mut App` for stateful widgets)
- Use `crossterm` `EventStream` for async-compatible terminal event polling
- Popups render `Clear` widget first, then content on top
- **Statusbar**: Keep it minimal — only essential keys (j/k, /, n, c, s, f, w, ?, q). All other keybindings belong in the `?` help overlay only. No theme name tag in the statusbar.

## CLI Pipeline (kube-log)
- JSON output is the default format; designed for piping into LLM tools
- Summary mode is default — suppress health checks, repeated lines, surface errors/warnings/lifecycle/novel patterns
- Classification priority: Error > Warning > HealthCheck > Lifecycle > Repeated > Novel > Normal (health check must precede lifecycle because health URLs like `/readyz` contain lifecycle words)
- The classify-filter-reduce pipeline lives in `kube-log-core` so both binaries can reuse it
- All CLI errors go to stderr as structured JSON for machine consumption
- Shell completions use `clap_complete` `CompleteEnv` (env-var based, not a subcommand): `source <(COMPLETE=bash kube-log)`
- Dynamic completers can't access sibling parsed args — namespace completer falls back to current kubeconfig context, pod completer falls back to "default" namespace

## K8s Interaction
- `kube::Client` is `Clone`-cheap -- clone into spawned tasks freely
- Recreate client on context switch via `Config::from_kubeconfig`
- Always set `tail_lines` on `LogParams` to avoid fetching entire log history
- Log stream tasks must respect the cancellation channel

# CODE REVIEW CHECKLIST

- Does the code handle errors without panicking?
- Are async operations properly awaited and not blocking the render loop?
- Is the K8s client lifecycle correct (no stale clients after context switch)?
- Are log streams properly cancelled when switching pods/containers?
- Does `cargo clippy --workspace --all-targets -- -D warnings` pass?
- Does `cargo test --workspace` pass?
- Are new features covered by tests?
- Is the terminal properly restored on all exit paths?
- Does the code prefer pattern matching (`match`) over `if-else` chains?
- Are GoF design patterns applied where they reduce lines of code (not aggressively)?

# OUT OF SCOPE / ANTI-PATTERNS

- GUI or web UI (this is a terminal application)
- Cluster management operations (deploy, scale, delete)
- Log persistence or database storage
- Resource metrics (CPU, memory)
- Multi-cluster simultaneous viewing
- Configuration file for the viewer itself (hardcoded defaults for v0.1)

# SUMMARY MANTRA

Connect to cluster. List pods. Stream logs. Search fast.
