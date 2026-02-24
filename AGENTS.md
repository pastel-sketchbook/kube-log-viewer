# ROLES AND EXPERTISE

This codebase operates with two distinct but complementary roles:

## Implementor Role

You are a senior Rust engineer building a high-performance TUI application. You implement changes with attention to error handling, async correctness, and terminal UX.

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
- Run `cargo clippy -- -D warnings` and `cargo test`

# SCOPE OF THIS REPOSITORY

This repository contains `kube-log-viewer`, a Rust TUI application for streaming and searching Kubernetes pod logs. It:

- **Connects** to Kubernetes clusters via `~/.kube/config`
- **Lists** namespaces and pods with status indicators
- **Streams** pod logs in real-time with follow mode
- **Filters** log lines by keyword search
- **Supports** multi-container pods with container selection
- **Switches** between Kubernetes contexts interactively

**Runtime requirements:**
- Any OS with Rust toolchain (1.75+)
- Access to a Kubernetes cluster (valid kubeconfig)

# ARCHITECTURE

```
kube-log-viewer/
├── Cargo.toml                              # Dependencies & binary config
├── src/
│   ├── main.rs                             # Entry point, terminal setup/teardown
│   ├── app.rs                              # App state, event loop, key handling
│   ├── event.rs                            # AppEvent enum
│   ├── k8s/
│   │   ├── mod.rs                          # Client creation (kubeconfig-based)
│   │   ├── contexts.rs                     # Context listing
│   │   ├── namespaces.rs                   # Namespace listing
│   │   ├── pods.rs                         # Pod listing + PodInfo extraction
│   │   └── logs.rs                         # Log streaming with cancellation
│   └── ui/
│       ├── mod.rs                          # Main layout, help overlay
│       ├── header.rs                       # Header bar
│       ├── pods.rs                         # Pod list panel
│       ├── logs.rs                         # Log viewer panel
│       ├── popup.rs                        # Popup overlays (pickers)
│       └── statusbar.rs                    # Keybinding hints
├── docs/rationale/                         # Architecture decision records
│   └── 0001_application_design.md          # Core design rationale
├── tests/                                  # Integration tests
├── TODO.md                                 # Phased implementation roadmap
└── .editorconfig                           # Editor settings
```

**Data flow:**
1. `main.rs` sets up the terminal and launches the async event loop
2. `App::run()` uses `tokio::select!` to multiplex terminal events, K8s channel events, and ticks
3. K8s operations run in `tokio::spawn` tasks, sending results via `mpsc::unbounded_channel<AppEvent>`
4. On each loop iteration, `ui::render()` draws the current state to the terminal via ratatui
5. Log streaming uses `Api::log_stream` with cooperative cancellation via `watch` channel

# CORE DEVELOPMENT PRINCIPLES

- **No Panics**: Never use `unwrap()` in non-test code. Use `?` with `anyhow::Context`. `.expect()` is permitted only when the invariant is logically guaranteed, with a safety comment explaining why.
- **Error Messages**: Provide actionable error messages with context. K8s connection errors should suggest checking kubeconfig.
- **Non-blocking Render**: Never perform blocking I/O in the render path. All K8s calls go through background tasks.
- **Graceful Degradation**: If a K8s call fails, show the error in the UI rather than crashing. The TUI must remain interactive.
- **Terminal Safety**: Always restore terminal state on exit, including on panic. Use a panic hook or `catch_unwind` wrapper.
- **Testing**: Unit tests for state transitions, pod info extraction, log filtering. Integration tests for event handling.

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
- Does `cargo clippy -- -D warnings` pass?
- Does `cargo test` pass?
- Are new features covered by tests?
- Is the terminal properly restored on all exit paths?

# OUT OF SCOPE / ANTI-PATTERNS

- GUI or web UI (this is a terminal application)
- Cluster management operations (deploy, scale, delete)
- Log persistence or database storage
- Resource metrics (CPU, memory)
- Multi-cluster simultaneous viewing
- Configuration file for the viewer itself (hardcoded defaults for v0.1)

# SUMMARY MANTRA

Connect to cluster. List pods. Stream logs. Search fast.
