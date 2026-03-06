# kube-log-viewer

A Rust workspace with two binaries for Kubernetes log analysis:

- **`kube-log-viewer`** (TUI) -- interactive terminal UI for streaming and searching pod logs
- **`kube-log`** (CLI) -- pipe-friendly CLI with anomaly detection, designed for LLM integration

## TUI Features

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

## CLI Features

The `kube-log` CLI runs a **classify-filter-reduce** pipeline over pod logs and outputs pre-triaged JSON for piping into LLM tools.

- **Log classification** -- Each line is classified as error, warning, lifecycle, health check, repeated, novel, or normal
- **Noise suppression** -- Health checks, repeated lines, and info-level noise are suppressed by default (troubleshoot mode)
- **Bounded summaries** -- Error buckets, warning buckets, timeline, restart events, and novel patterns, capped in size regardless of log volume
- **Auto-discovery** -- Omit `--pod` to analyze all pods in the namespace
- **Sidecar filtering** -- Automatically skips well-known sidecars (istio-proxy, linkerd-proxy, vault-agent, etc.) in multi-container pods
- **Namespace suggestions** -- Suggests similar namespace names when no pods are found
- **Follow mode** -- Stream classified lines as JSONL in real-time with `-f`
- **Output formats** -- JSON (default), JSONL, or plain text
- **Shell completions** -- Dynamic tab-completion for contexts, namespaces, and pods
- **Structured errors** -- All errors go to stderr as JSON for machine consumption

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
# Build both binaries from source
cargo build --release

# Or install each binary individually
cargo install --path crates/kube-log-tui    # installs kube-log-viewer
cargo install --path crates/kube-log-cli    # installs kube-log
```

Pre-built binaries are available on the [Releases](https://github.com/pastel-sketchbook/kube-log-viewer/releases) page.

## Usage

### TUI

```sh
kube-log-viewer
```

No arguments required. Reads the current kubeconfig and connects to the active context.

### CLI

```sh
# Analyze all pods in the current namespace (default: troubleshoot mode)
kube-log

# Analyze a specific pod, last 15 minutes
kube-log --pod payments-7f8d --time-range 15m

# Pipe into an LLM for diagnosis
kube-log -n production --pod api-server | opencode run "Diagnose this incident"

# Follow mode — stream classified JSONL in real-time
kube-log --pod payments-7f8d -f

# Show all lines (disable noise suppression)
kube-log --pod payments-7f8d --all

# Filter to specific classes
kube-log --pod payments-7f8d --include error,warning,lifecycle

# List pods with status as JSON
kube-log pods -n kube-system

# List contexts / namespaces
kube-log contexts
kube-log namespaces
kube-log -n              # shorthand for namespaces

# Plain text output
kube-log --pod payments-7f8d --format plain
```

#### Subcommands

| Command | Alias | Description |
|---------|-------|-------------|
| `logs` | *(default)* | Fetch and analyze pod logs |
| `pods` | `po` | List pods with status |
| `contexts` | `ctx` | List available K8s contexts |
| `namespaces` | `ns` | List namespaces |

#### Key Flags (logs)

| Flag | Short | Description |
|------|-------|-------------|
| `--pod` | `-p` | Pod name(s). Omit to analyze all pods in namespace |
| `--namespace` | `-n` | Namespace (default: current from kubeconfig) |
| `--context` | | Kubernetes context (default: current) |
| `--container` | `-c` | Container name (auto-selects app containers, skips sidecars) |
| `--follow` | `-f` | Stream classified lines as JSONL |
| `--lines` | | Number of recent lines per pod (default: 1000) |
| `--time-range` | | Time window: `5m`, `15m`, `1h`, `6h`, `24h`, etc. |
| `--search` | `-s` | Filter lines by substring match |
| `--all` | | Disable filtering, show all lines |
| `--verbose` | | Include normal lines (still suppress health checks) |
| `--include` | | Comma-separated classes: `error,warning,lifecycle,novel,normal,healthcheck` |
| `--format` | | Output format: `json` (default), `jsonl`, `plain` |

#### Shell Completions

```sh
# Bash
source <(COMPLETE=bash kube-log)

# Zsh
source <(COMPLETE=zsh kube-log)

# Fish
COMPLETE=fish kube-log | source
```

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
crates/
├── kube-log-core/           # Shared library crate
│   ├── k8s/                 # Client, contexts, namespaces, pods, log streaming
│   ├── classify.rs          # Log line classifier (error, warning, lifecycle, etc.)
│   ├── filter.rs            # Noise suppression, class filtering, collapse groups
│   ├── reduce.rs            # Bounded summary: error buckets, timeline, restarts
│   ├── export.rs            # JSON/JSONL/plain formatters, LLM hint generation
│   ├── parse.rs             # Timestamp regex, JSON flattening
│   └── types.rs             # Pipeline types: LineClass, ClassifiedLine, Summary
├── kube-log-tui/            # TUI binary crate (kube-log-viewer)
│   ├── app.rs               # App state, event loop, key handling
│   ├── event.rs             # AppEvent enum
│   ├── prefs.rs             # Theme persistence
│   └── ui/                  # ratatui rendering (header, pods, logs, popup, etc.)
└── kube-log-cli/            # CLI binary crate (kube-log)
    ├── cli.rs               # Clap argument definitions
    ├── output.rs            # Pipeline: fetch → classify → filter → reduce → export
    └── complete.rs          # Dynamic shell completers (context, namespace, pod)
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
