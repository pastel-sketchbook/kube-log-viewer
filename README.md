# kube-log-viewer

A terminal UI for streaming and searching Kubernetes pod logs.

## Features

- **Namespace selection** -- Browse and switch between namespaces
- **Pod listing** -- List pods with status indicators (Running, Pending, Failed, etc.)
- **Live log streaming** -- Stream logs from a selected pod/container in real-time
- **Log search/filter** -- Search and filter log lines by keyword
- **Multi-container support** -- Select specific containers within multi-container pods
- **Context switching** -- Switch between Kubernetes contexts from within the TUI

## Requirements

- Rust toolchain (1.75+)
- Access to a Kubernetes cluster (valid `~/.kube/config`)

## Installation

```sh
# Build from source
cargo build --release

# Or install directly
cargo install --path .
```

## Usage

```sh
kube-log-viewer
```

No arguments required. Reads the current kubeconfig and connects to the active context.

## Keybindings

| Key | Action |
|-----|--------|
| `j`/`k` or `↑`/`↓` | Navigate up/down |
| `Enter` | Select pod, start log stream |
| `Tab` | Switch focus (Pods / Logs) |
| `n` | Switch namespace |
| `c` | Switch context |
| `s` | Switch container |
| `/` | Search/filter logs |
| `f` | Toggle follow mode |
| `w` | Toggle line wrap |
| `g` / `G` | Scroll to top / bottom (logs) |
| `PgUp` / `PgDn` | Page up / down (logs) |
| `?` | Toggle help overlay |
| `Esc` | Close popup / exit search / clear filter |
| `q` | Quit |
| `Ctrl+C` | Force quit |

## Architecture

```
src/
├── main.rs          # Entry point, terminal setup/teardown
├── app.rs           # App state, event loop, key handling
├── event.rs         # AppEvent enum (K8s data, terminal input)
├── k8s/             # Kubernetes interaction layer
│   ├── mod.rs       # Client creation (kubeconfig-based)
│   ├── contexts.rs  # Context listing from kubeconfig
│   ├── namespaces.rs# Namespace listing
│   ├── pods.rs      # Pod listing with status extraction
│   └── logs.rs      # Log streaming with cancellation
└── ui/              # TUI rendering (ratatui)
    ├── mod.rs       # Main layout, help overlay
    ├── header.rs    # Header bar (context, namespace)
    ├── pods.rs      # Pod list panel
    ├── logs.rs      # Log viewer panel
    ├── popup.rs     # Popup overlays (pickers)
    └── statusbar.rs # Keybinding hints
```

See `docs/rationale/` for architecture decision records.

## Development

```sh
# Run in development
cargo run

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings
```

## License

MIT
