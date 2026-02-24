# kube-log-viewer

A terminal UI for streaming and searching Kubernetes pod logs.

## Features

- **Namespace selection** -- Browse and switch between namespaces
- **Pod listing** -- Live-updating pod list with status indicators (Running, Pending, Failed, etc.)
- **Live log streaming** -- Stream logs from a selected pod/container in real-time with follow mode
- **Log search/filter** -- Case-insensitive search and filter log lines by keyword
- **Multi-container support** -- Select specific containers within multi-container pods
- **Context switching** -- Switch between Kubernetes contexts from within the TUI
- **JSON log formatting** -- Pretty-print JSON log lines with syntax highlighting (on by default, toggle with `J`)
- **Timestamp modes** -- Cycle between UTC, Local, and Relative timestamp display (`T`)
- **Time range filtering** -- Filter logs to a time window (last 5m, 15m, 30m, 1h, 6h, 24h)
- **Multi-stream viewing** -- Stream logs from up to 4 pods simultaneously
  - **Merged mode** -- All streams interleaved chronologically, color-coded by pod
  - **Split mode** -- Stacked horizontal panes, one per stream, with independent scroll
- **Log export** -- Export filtered logs to file (Plain Text, JSON, or CSV) with metadata header
- **Themes** -- Multiple color themes, cycle with `t`
- **Azure (AKS) auth** -- Automatic `az login` when credentials expire

## Prerequisites

- Rust toolchain (1.75+)
- Access to a Kubernetes cluster (valid `~/.kube/config`)
- [Azure CLI](https://learn.microsoft.com/en-us/cli/azure/install-azure-cli) (`az`) -- required for authenticating to AKS clusters
- [kubelogin](https://github.com/Azure/kubelogin) -- required for AAD/Entra ID-based AKS authentication

### Azure (AKS) setup

```sh
# Login to Azure
az login

# Get credentials for your AKS cluster (merges into ~/.kube/config)
az aks get-credentials --resource-group <resource-group> --name <cluster-name>

# If your cluster uses AAD/Entra ID auth, convert the kubeconfig for kubelogin
kubelogin convert-kubeconfig -l azurecli
```

After this, `~/.kube/config` will contain the context for your cluster and `kube-log-viewer` can connect to it.

## Installation

```sh
# Build from source
cargo build --release

# Or install directly
cargo install --path .
```

Pre-built binaries are available on the [Releases](https://github.com/pastel-sketchbook/kube-log-viewer/releases) page.

## Usage

```sh
kube-log-viewer
```

No arguments required. Reads the current kubeconfig and connects to the active context.

## Keybindings

Press `?` inside the application to see the full help overlay.

### Navigation

| Key | Action |
|-----|--------|
| `j`/`k` or `Up`/`Down` | Navigate up/down |
| `Enter` | Select pod, start log stream |
| `Tab` | Switch focus (Pods / Logs) |
| `g` / `G` | Scroll to top / bottom (logs) |
| `PgUp` / `PgDn` | Page up / down (logs) |

### Actions

| Key | Action |
|-----|--------|
| `n` | Switch namespace |
| `c` | Switch context |
| `s` | Switch container |
| `/` | Search/filter logs |
| `f` | Toggle follow mode |
| `w` | Toggle wide log panel |
| `W` | Toggle line wrap |
| `J` | Toggle JSON formatting |
| `T` | Cycle timestamp mode (UTC / Local / Relative) |
| `R` | Set time range filter |
| `t` | Cycle theme |
| `E` | Export logs to file |

### Multi-stream

| Key | Action |
|-----|--------|
| `M` | Add currently selected pod as additional stream (merged mode) |
| `V` | Cycle view: Single / Merged / Split |
| `X` | Remove most recently added stream |
| `1`-`4` | Switch active pane (split mode) |

### General

| Key | Action |
|-----|--------|
| `?` | Toggle help overlay |
| `Esc` | Close popup / exit search / clear filter |
| `q` | Quit |
| `Ctrl+C` | Force quit |

## Architecture

```
src/
├── main.rs          # Entry point, terminal setup/teardown
├── lib.rs           # Public module re-exports
├── app.rs           # App state, event loop, key handling
├── event.rs         # AppEvent enum (K8s data, terminal input)
├── k8s/             # Kubernetes interaction layer
│   ├── mod.rs       # Client creation (kubeconfig-based)
│   ├── contexts.rs  # Context listing from kubeconfig
│   ├── namespaces.rs# Namespace listing
│   ├── pods.rs      # Pod listing + live watcher
│   └── logs.rs      # Log streaming with cancellation
└── ui/              # TUI rendering (ratatui)
    ├── mod.rs       # Main layout, help overlay
    ├── header.rs    # Header bar (context, namespace)
    ├── pods.rs      # Pod list panel
    ├── logs.rs      # Log viewer panel (single, merged, split)
    ├── popup.rs     # Popup overlays (pickers)
    ├── statusbar.rs # Keybinding hints
    └── theme.rs     # Color themes
```

See `docs/rationale/` for architecture decision records.

## Development

Requires [Task](https://taskfile.dev/) for running development commands.

```sh
# Format, lint, and test
task check:all

# Build and install locally
task install

# Run in development
task run

# Run tests only
task test

# Build optimized release binary
task build:release
```

## License

MIT
