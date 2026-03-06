# ADR-0007: LLM CLI — Dual Interface with Troubleshoot-First Design

**Date:** 2026-03-06
**Status:** Proposed

## Context

The TUI is designed for humans scrolling through logs interactively. But a growing class of consumers — LLM agents, CI pipelines, MCP tool servers — need the same Kubernetes log data in a machine-readable, token-efficient format. Today, users bridge this gap with `kubectl logs ... | pbcopy` and manual pasting into chat windows, wasting tokens on health checks, repeated noise, and unstructured text that the LLM must re-parse.

The core K8s layer (`k8s/`), log parsing (`TIMESTAMP_RE`, `format_json_line`), and filtering (`filtered_log_lines`) are already implemented but coupled to the TUI event loop and ratatui types. A second binary can reuse this logic if we extract it into a shared library crate.

### Design Philosophy: Troubleshoot Mode as Default

The CLI's primary persona is **an LLM debugging a production incident**. This inverts the typical log viewer assumption (show everything, let the human filter). Instead:

- **Default is reduction.** Health checks, repeated identical lines, and info-level noise are suppressed unless explicitly requested.
- **Anomalies surface automatically.** Errors, warnings, crash loops, status transitions, and novel log patterns are prioritized.
- **Output is pre-parsed.** Timestamps, levels, and structured fields are extracted so the LLM spends tokens on reasoning, not parsing.
- **Token budget is respected.** A map-reduce pipeline compresses thousands of log lines into a bounded summary, with drill-down available on demand.

## Decision

### 1. Workspace Restructure

Split the single crate into a Cargo workspace with three members:

```
kube-log-viewer/                    (workspace root)
├── Cargo.toml                      (workspace definition)
├── crates/
│   ├── kube-log-core/              (library)
│   │   ├── k8s/                    (client, contexts, namespaces, pods, log streaming)
│   │   ├── parse.rs                (TIMESTAMP_RE, parse_log_timestamp, format_json_line)
│   │   ├── filter.rs               (search, time-range, health-check, dedup, anomaly)
│   │   ├── classify.rs             (log level detection, anomaly scoring)
│   │   ├── reduce.rs               (map-reduce pipeline, summary generation)
│   │   ├── export.rs               (plain text, JSON, CSV writers)
│   │   └── types.rs                (TaggedLine, PodInfo, ClassifiedLine, Summary, etc.)
│   │
│   ├── kube-log-tui/               (binary — existing TUI)
│   │   ├── app.rs, ui/, prefs.rs   (unchanged behavior, depends on core)
│   │   └── ...
│   │
│   └── kube-log-cli/               (binary — LLM-facing CLI)
│       ├── main.rs                 (clap args, output dispatch)
│       ├── output.rs               (JSON-lines formatter, summary formatter)
│       └── session.rs              (interactive stdin/stdout session mode)
```

### 2. Smart Anomaly Detection Pipeline

The CLI processes logs through a **classify-filter-reduce** pipeline before output. This replaces the TUI's passive "show everything, let the human scroll" model with active triage.

#### Stage 1: Classify (`classify.rs`)

Every log line is analyzed and tagged with a `LineClass`:

```rust
pub enum LineClass {
    /// Error-level: panics, stack traces, HTTP 5xx, connection refused, OOM, etc.
    Error,
    /// Warning-level: retries, timeouts, deprecation notices, HTTP 4xx
    Warning,
    /// State transition: pod started, container ready, graceful shutdown, config reload
    Lifecycle,
    /// First occurrence of a message pattern not seen before in this stream
    Novel,
    /// Health check: kube-probe, liveness/readiness/startup probe responses
    HealthCheck,
    /// Repeated: structurally identical to a line already seen (modulo timestamp/request-id)
    Repeated { count: u32, canonical: String },
    /// Normal: info-level, routine operational log
    Normal,
}
```

Classification rules (applied in priority order):

| Rule | Detects | LineClass |
|------|---------|-----------|
| Substring: `ERROR`, `FATAL`, `PANIC`, `panic`, HTTP `5xx` status | Hard errors | `Error` |
| Substring: `WARN`, `TIMEOUT`, `retry`, HTTP `4xx` status | Soft errors | `Warning` |
| Substring: `started`, `ready`, `shutdown`, `SIGTERM`, `pulling image` | State changes | `Lifecycle` |
| Substring: `kube-probe`, `/healthz`, `/readyz`, `/livez` | K8s probes | `HealthCheck` |
| Structural dedup: normalize away timestamps, UUIDs, request IDs, IP addresses; compare against seen-set | Noise lines | `Repeated` |
| First-seen canonical form | Unseen pattern | `Novel` |
| Everything else | Routine | `Normal` |

**Structural dedup** is the key innovation. Two lines like:

```
2026-03-06T10:15:23Z INFO  GET /api/users request_id=a1b2c3 latency=12ms
2026-03-06T10:15:24Z INFO  GET /api/users request_id=d4e5f6 latency=14ms
```

produce the same canonical form after normalizing timestamps, UUIDs, and numeric values:

```
INFO GET /api/users request_id=<id> latency=<num>ms
```

The second occurrence is classified as `Repeated { count: 2, canonical: "..." }` and suppressed in default output.

#### Stage 2: Filter (Default Troubleshoot Mode)

In troubleshoot mode (the default), the CLI **keeps** only:

- `Error` — always shown
- `Warning` — always shown
- `Lifecycle` — always shown (pod starts, restarts, crashes are diagnostic gold)
- `Novel` — shown (first-seen patterns often indicate the root cause)

And **suppresses**:

- `HealthCheck` — dropped entirely
- `Repeated` — collapsed into a single summary line: `[... 847 similar lines omitted: INFO GET /api/users ...]`
- `Normal` — dropped unless `--verbose` flag

Users can override with `--all` to disable filtering, or `--include=normal,healthcheck` for fine-grained control.

#### Stage 3: Reduce (Map-Reduce for Token Economy)

The reduce stage compresses the classified output into a bounded token budget. This is critical: a pod producing 10,000 lines/minute can easily overwhelm an LLM context window.

**Map phase** (per-line, streaming):

Each `ClassifiedLine` produces a lightweight record:

```rust
pub struct MappedLine {
    pub timestamp: Option<DateTime<Utc>>,
    pub level: LineClass,
    pub source: String,
    pub canonical: String,         // normalized form for dedup
    pub raw: String,               // original text
    pub fields: Option<JsonMap>,   // parsed structured fields (if JSON)
}
```

**Reduce phase** (aggregation):

```rust
pub struct Summary {
    pub time_range: (DateTime<Utc>, DateTime<Utc>),
    pub total_lines: u64,
    pub suppressed_lines: u64,
    pub error_count: u64,
    pub warning_count: u64,
    pub pods: Vec<PodSummary>,
    pub top_errors: Vec<ErrorBucket>,       // deduplicated, ranked by frequency
    pub top_warnings: Vec<ErrorBucket>,
    pub timeline: Vec<TimelineEntry>,       // error rate per time bucket
    pub novel_patterns: Vec<String>,        // first-seen patterns (potential root cause)
    pub restart_events: Vec<RestartEvent>,  // pod restart history
}

pub struct ErrorBucket {
    pub canonical: String,     // normalized error pattern
    pub count: u64,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub sample: String,        // one raw example for the LLM to read
}

pub struct TimelineEntry {
    pub bucket_start: DateTime<Utc>,
    pub error_count: u64,
    pub warning_count: u64,
    pub novel_count: u64,
}
```

The reduce output has **bounded size** regardless of input volume:

- `top_errors`: capped at 20 buckets
- `top_warnings`: capped at 20 buckets
- `timeline`: one entry per minute, capped at 60 (last hour)
- `novel_patterns`: capped at 30
- Each bucket carries exactly one `sample` raw line

This guarantees the summary fits within ~2K-4K tokens even for pods producing millions of lines.

### 3. Output Format: JSON as Default

All CLI output is JSON. No flags needed for the common case.

**Log lines mode** (default when streaming or fetching lines):

```jsonl
{"ts":"2026-03-06T10:15:23Z","pod":"payments-7f8d","container":"app","class":"error","level":"ERROR","msg":"connection refused to db:5432","raw":"2026-03-06T10:15:23Z ERROR connection refused to db:5432","fields":{"target":"db:5432"}}
{"ts":"2026-03-06T10:15:24Z","pod":"payments-7f8d","container":"app","class":"warning","level":"WARN","msg":"retrying in 5s","raw":"..."}
{"_collapsed":true,"count":847,"canonical":"INFO GET /api/users request_id=<id> latency=<num>ms","sample":"2026-03-06T10:15:25Z INFO GET /api/users request_id=a1b2c3 latency=12ms"}
```

Each line is a self-contained JSON object (JSON-lines format). Fields:

| Field | Always present | Description |
|-------|----------------|-------------|
| `ts` | When parseable | ISO 8601 UTC timestamp |
| `pod` | Yes | Source pod name |
| `container` | When known | Container name |
| `class` | Yes | `error` / `warning` / `lifecycle` / `novel` / `normal` |
| `level` | When detected | Original level string |
| `msg` | When extractable | Human-readable message (from JSON `msg`/`message` field, or the non-timestamp portion of the raw line) |
| `raw` | Yes | Verbatim log line from K8s API |
| `fields` | When JSON | Remaining structured fields as a nested object |
| `_collapsed` | Only for collapsed groups | Indicates a suppressed-lines summary |
| `count` | Only for collapsed | Number of suppressed repetitions |
| `canonical` | Only for collapsed | Normalized pattern |
| `sample` | Only for collapsed | One verbatim example |

**Summary mode** (`--summary` or when output is not a TTY and `--follow` is not set):

```json
{
  "summary": {
    "time_range": ["2026-03-06T10:00:00Z", "2026-03-06T10:15:30Z"],
    "total_lines": 48392,
    "suppressed_lines": 47201,
    "error_count": 23,
    "warning_count": 147,
    "pods": [
      {"name": "payments-7f8d", "status": "Running", "restarts": 2}
    ],
    "top_errors": [
      {
        "canonical": "ERROR connection refused to db:<port>",
        "count": 18,
        "first_seen": "2026-03-06T10:12:01Z",
        "last_seen": "2026-03-06T10:15:23Z",
        "sample": "2026-03-06T10:15:23Z ERROR connection refused to db:5432"
      }
    ],
    "timeline": [
      {"bucket": "2026-03-06T10:12:00Z", "errors": 3, "warnings": 12, "novel": 1},
      {"bucket": "2026-03-06T10:13:00Z", "errors": 8, "warnings": 45, "novel": 0},
      {"bucket": "2026-03-06T10:14:00Z", "errors": 7, "warnings": 52, "novel": 0},
      {"bucket": "2026-03-06T10:15:00Z", "errors": 5, "warnings": 38, "novel": 0}
    ],
    "novel_patterns": [
      "ERROR connection refused to db:<port>",
      "WARN pool exhausted, queuing request"
    ],
    "restart_events": [
      {"pod": "payments-7f8d", "at": "2026-03-06T10:12:00Z", "reason": "CrashLoopBackOff"}
    ]
  },
  "lines": [
    {"ts": "...", "class": "error", "msg": "...", "raw": "..."}
  ]
}
```

The `summary` block gives the LLM a compressed situational overview. The `lines` array contains only the classified-and-filtered lines (errors, warnings, lifecycle, novel). An LLM receiving this output can immediately reason about the incident without scanning thousands of routine lines.

### 4. CLI Interface

```
kube-log <subcommand> [flags]

SUBCOMMANDS:
    logs        Fetch and analyze pod logs (default)
    pods        List pods with status
    contexts    List available K8s contexts
    namespaces  List namespaces

LOGS FLAGS:
    --context <name>          K8s context (default: current)
    --namespace <name>        Namespace (default: current)
    --pod <name>              Pod name (required, repeatable for multi-pod)
    --container <name>        Container name (optional)
    --lines <n>               Number of recent lines to fetch (default: 1000)
    --follow                  Stream continuously (JSON-lines to stdout)
    --time-range <duration>   Only include lines from last N (e.g. 15m, 1h, 6h)
    --search <query>          Filter lines by substring match

    --summary                 Output summary + filtered lines (default for non-TTY)
    --all                     Disable troubleshoot filtering, show all lines
    --verbose                 Include Normal-class lines (but still suppress health checks)
    --include <classes>       Comma-separated: error,warning,lifecycle,novel,normal,healthcheck
    --format <fmt>            json (default), jsonl, plain
```

No `--format` flag needed for the common case — JSON is default. `--format plain` available for `grep`-style workflows or piping into tools that expect plain text.

### 5. Shell Completion with Live K8s Lookups

Tab completion is critical for a CLI that targets Kubernetes resources with long, generated names. Nobody wants to type `payments-deployment-7f8d9b6c4a-x2k9p` by hand. The CLI supports two layers of completion:

#### Static Completions (subcommands, flags)

Generated at build time via `clap_complete`. The user registers them once:

```bash
# bash — add to ~/.bashrc
eval "$(kube-log completions bash)"

# zsh — add to ~/.zshrc
eval "$(kube-log completions zsh)"

# fish
kube-log completions fish | source
```

This covers subcommands (`logs`, `pods`, `contexts`, `namespaces`), all flags (`--context`, `--namespace`, `--pod`, `--follow`, etc.), and enum values (`--format json|jsonl|plain`, `--include error,warning,...`).

#### Dynamic Completions (live K8s resources)

This is where it gets useful. When the user hits Tab on a flag that takes a K8s resource name, the CLI queries the cluster in real-time:

```bash
kube-log logs --context <TAB>
# → prod-aks  staging-aks  dev-aks

kube-log logs --context prod-aks --namespace <TAB>
# → api  payments  gateway  monitoring  kube-system

kube-log logs --namespace api --pod <TAB>
# → payments-7f8d9b6c4a-x2k9p  gateway-3a2b1c-m8n7  users-5d4e3f-q1w2

kube-log logs --namespace api --pod payments-7f8d9b6c4a-x2k9p --container <TAB>
# → app  istio-proxy  fluentbit
```

Each flag cascades: `--namespace` completion uses the `--context` value (if already provided), `--pod` uses both `--context` and `--namespace`, and `--container` uses all three.

**Implementation via `clap_complete` v4+ `CompleteEnv`:**

`clap_complete` v4 supports runtime custom completers per argument. Each completer is a function that receives the partial command line and returns candidates:

```rust
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};

fn context_completer(current: &std::ffi::OsStr) -> Vec<CompletionCandidate> {
    // Reads ~/.kube/config synchronously — fast, no network call
    let Ok((contexts, _)) = kube_log_core::k8s::contexts::load_contexts() else {
        return vec![];
    };
    let prefix = current.to_string_lossy();
    contexts
        .into_iter()
        .filter(|c| c.starts_with(prefix.as_ref()))
        .map(|c| CompletionCandidate::new(c))
        .collect()
}

fn namespace_completer(current: &std::ffi::OsStr) -> Vec<CompletionCandidate> {
    // Requires K8s API call — runs a small tokio runtime
    // Uses --context from already-parsed args, falls back to current context
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all().build().unwrap();
    let Ok(namespaces) = rt.block_on(
        kube_log_core::k8s::namespaces::list_namespaces(context)
    ) else {
        return vec![];
    };
    // ... filter by prefix, return candidates
}

fn pod_completer(current: &std::ffi::OsStr) -> Vec<CompletionCandidate> {
    // Uses --context and --namespace from already-parsed args
    // Returns pod names, with status as help text:
    //   payments-7f8d  (Running, 0 restarts)
    //   gateway-3a2b   (CrashLoopBackOff, 5 restarts)
    // ...
}

fn container_completer(current: &std::ffi::OsStr) -> Vec<CompletionCandidate> {
    // Uses --context, --namespace, --pod from already-parsed args
    // Lists containers within the selected pod
    // ...
}
```

These are wired into the clap `Arg` definitions:

```rust
Arg::new("context")
    .long("context")
    .add(ArgValueCompleter::new(context_completer))

Arg::new("namespace")
    .long("namespace")
    .add(ArgValueCompleter::new(namespace_completer))

Arg::new("pod")
    .long("pod")
    .add(ArgValueCompleter::new(pod_completer))

Arg::new("container")
    .long("container")
    .add(ArgValueCompleter::new(container_completer))
```

#### Completion Latency Budget

Dynamic completions hit real infrastructure. Latency targets:

| Flag | Data source | Expected latency | Notes |
|------|-------------|------------------|-------|
| `--context` | `~/.kube/config` file | <5ms | Local file read, no network |
| `--namespace` | K8s API `GET /api/v1/namespaces` | 50-200ms | One API call, small payload |
| `--pod` | K8s API `GET /api/v1/namespaces/{ns}/pods` | 50-300ms | One API call, size depends on namespace |
| `--container` | K8s API `GET /api/v1/namespaces/{ns}/pods/{pod}` | 50-150ms | Single pod metadata |

Context completion is instant (local file). Namespace and pod completions add a perceptible but acceptable delay on first Tab press. Shells like zsh and fish display a spinner or "loading" indicator during completion, so sub-300ms is fine.

**No caching.** Completions always query live state. Pod lists in Kubernetes change frequently (deployments, scaling, crash loops), and stale completions that suggest non-existent pods are worse than a 200ms wait. The kubeconfig read for context completion is already effectively instant.

#### Rich Completion Metadata

Where shells support it (zsh, fish), completions include descriptive help text:

```
$ kube-log logs --pod <TAB>
payments-7f8d9b6c4a-x2k9p    Running (2 restarts)
gateway-3a2b1c-m8n7           Running (0 restarts)
users-5d4e3f-q1w2             CrashLoopBackOff (8 restarts)
```

This surfaces pod health directly in the completion menu — the user (or an interactive LLM tool) can see which pods are unhealthy before selecting. `CompletionCandidate::new(name).help(status_string)` provides this in `clap_complete`.

#### Subcommand Shorthand

Frequent operations get short aliases that also complete:

```bash
kube-log ctx <TAB>     # alias for 'kube-log contexts'
kube-log ns <TAB>      # alias for 'kube-log namespaces'
kube-log po <TAB>      # alias for 'kube-log pods --namespace ...'
```

These mirror `kubectl` muscle memory (`kubectl get po`, `kubectl get ns`).

### 6. Authentication Scope

The CLI assumes the user has a valid kubeconfig with active credentials. No `az login` auto-recovery, no browser-based auth flow. If credentials are expired, the CLI emits a structured error and exits non-zero:

```json
{"error": "unauthorized", "message": "Kubernetes API returned 401. Run 'az login' or refresh your kubeconfig credentials.", "context": "prod-cluster"}
```

This keeps the CLI stateless and predictable for automation.

### 7. Reuse Strategy — Decoupling Steps

Three targeted refactors unlock the shared library, ordered by impact:

**Step 1: Return streams, not channels.**

`k8s::logs::stream_logs` and `k8s::pods::watch_pods` currently take `mpsc::UnboundedSender<AppEvent>`, coupling them to the app event enum. Refactor both to return `impl Stream<Item = Result<T>>`:

```rust
// Before (coupled to AppEvent)
pub async fn stream_logs(..., tx: mpsc::UnboundedSender<AppEvent>) -> Result<()>

// After (generic, reusable)
pub fn stream_logs(...) -> impl Stream<Item = Result<String>>
```

The TUI adapter maps stream items into `AppEvent::LogLine(...)` via `StreamExt::map`. The CLI consumes the stream directly.

**Step 2: Move parse utilities out of `ui/`.**

`TIMESTAMP_RE`, `parse_log_timestamp`, and `format_json_line` are domain logic living in `src/ui/logs.rs`. Move them to `kube-log-core::parse`. Zero behavior change. Eliminates the coupling that currently forces `filtered_log_lines()` to import from the UI layer.

**Step 3: Split `LogStreamHandle`.**

Separate domain fields (`pod_name`, `container`, `cancel_tx`) from TUI fields (`scroll_offset`, `follow_mode`) into two structs. The core crate owns `StreamHandle`; the TUI crate wraps it with `PaneState`.

### 8. MCP Tool Server Mapping

The session mode (`kube-log --session`) naturally maps to an MCP tool server. The classify-filter-reduce pipeline means every tool response is pre-triaged:

| MCP Tool | Maps to | Returns |
|----------|---------|---------|
| `kube_list_contexts` | `k8s::contexts::load_contexts` | `Vec<String>` |
| `kube_list_pods` | `k8s::pods::list_pods` | `Vec<PodInfo>` as JSON |
| `kube_get_logs` | stream + classify + reduce | `Summary` + filtered lines |
| `kube_stream_logs` | stream + classify (no reduce) | JSON-lines stream |
| `kube_get_raw_logs` | stream only, `--all` | Raw lines for when the LLM needs full context |

The default `kube_get_logs` tool returns the **summary** — the LLM gets a 2K-token incident overview and can drill down with `kube_get_raw_logs` if needed.

### 9. LLM Post-Processor Integration: Pipe is King

The primary use case is: fetch and triage logs locally with `kube-log`, then pipe the reduced output into an LLM for diagnosis. Two target LLM agents: **GitHub Copilot CLI** and **OpenCode**.

#### Prior Art: zig-saju

This pattern is proven in [`zig-saju`](../../../langs/zig/zig-saju), a Zig astrology engine that follows the same philosophy:

- **Compact text as default output** — structured `##`-delimited sections, dense inline data, no decoration. Designed from day one to be piped into an LLM.
- **`opencode run` as the post-processor** — the web UI spawns `opencode run --format json`, pipes saju JSON via stdin, and streams the LLM's interpretation back to the browser.
- **No MCP, no custom tools, no ceremony** — just `saju ... | opencode run "interpret this"`. The pipe is the entire integration layer.
- **No TTY detection** — output is identical whether viewed in a terminal or piped. The default format is already LLM-friendly, so there's nothing to switch.

`kube-log` follows this exact philosophy. The pipe is the interface.

#### Design Principle: Pipe-First, Everything Else Later

```
kube-log logs --pod payments-7f8d --time-range 15m \
  | opencode run "Diagnose this Kubernetes incident"
```

That's it. That's the integration. Everything else is optional future work.

Why piping wins:

1. **Zero config.** No MCP server registration, no custom tool files, no JSON-RPC protocol. Install `kube-log`, pipe to your LLM CLI.
2. **Composable.** Standard Unix pipes work with any LLM CLI — current and future. `opencode run`, `copilot -p`, `llm`, `aichat`, whatever ships next year.
3. **Debuggable.** `kube-log logs --pod foo > /tmp/debug.json` — inspect the exact input the LLM will see. Try that with MCP.
4. **Proven.** The zig-saju project already runs this in production. The pattern works.

#### The Pipe Workflow

**Step 1: kube-log fetches, classifies, reduces.**

The classify-filter-reduce pipeline (Sections 2-3) runs locally. Health checks, repeated lines, and info-level noise are stripped. Errors, warnings, lifecycle events, and novel patterns survive. The output is a self-contained JSON document with everything the LLM needs:

```json
{
  "_hint": "Kubernetes pod logs from context 'prod-aks', namespace 'api'. Troubleshoot mode: only errors, warnings, lifecycle events, and novel patterns shown. Health checks and repeated lines suppressed. 47201 of 48392 lines omitted.",
  "summary": {
    "time_range": ["2026-03-06T10:00:00Z", "2026-03-06T10:15:30Z"],
    "total_lines": 48392,
    "suppressed_lines": 47201,
    "error_count": 23,
    "warning_count": 147,
    "top_errors": [
      {
        "pattern": "connection refused to db:<port>",
        "count": 18,
        "first_seen": "2026-03-06T10:12:01Z",
        "last_seen": "2026-03-06T10:15:23Z",
        "sample": "2026-03-06T10:15:23Z ERROR connection refused to db:5432"
      }
    ],
    "restart_events": [
      { "pod": "payments-7f8d", "at": "2026-03-06T10:12:00Z", "reason": "CrashLoopBackOff" }
    ]
  },
  "lines": [
    { "ts": "2026-03-06T10:12:01Z", "pod": "payments-7f8d", "class": "error", "msg": "connection refused to db:5432" },
    { "ts": "2026-03-06T10:12:02Z", "pod": "payments-7f8d", "class": "lifecycle", "msg": "container restarting" }
  ]
}
```

The `_hint` field is a natural-language preamble — a system-prompt injection that tells the LLM what it's looking at, what's been filtered, and the scale of reduction. The LLM doesn't waste tokens figuring out the data format.

**Step 2: Pipe to the LLM.**

**OpenCode** (preferred — supports stdin piping, streaming output):

```bash
# One-shot incident diagnosis
kube-log logs --pod payments-7f8d --time-range 15m \
  | opencode run "Diagnose this Kubernetes incident. What is the root cause?"

# Multi-pod investigation
kube-log logs --pod payments-7f8d --pod gateway-3a2b --time-range 1h \
  | opencode run "These are logs from two related services. Correlate the errors."

# Save first, iterate in conversation
kube-log logs --pod payments-7f8d > /tmp/incident.json
opencode run -f /tmp/incident.json "Diagnose. Ask me if you need more context."
```

**Copilot CLI**:

```bash
# Programmatic one-shot
kube-log logs --pod payments-7f8d --time-range 15m \
  | copilot -p "Diagnose this Kubernetes incident. What is the root cause?"

# Interactive session with kube-log as an allowed shell tool
copilot --allow-tool 'shell(kube-log)'
# → Copilot can now call kube-log subcommands on its own during conversation
```

**Step 3: LLM reasons over pre-triaged data.**

The LLM receives ~2K-4K tokens of structured incident data instead of 48K lines of raw logs. It can immediately identify:
- The error pattern (connection refused to database)
- The timeline (started at 10:12, ongoing)
- The impact (pod crash-looping, 8 restarts)
- The correlation (errors began after a specific timestamp)

No parsing. No scrolling through health checks. The reduce pipeline already did the triage.

#### Copilot CLI: Shell Tool Escalation

When using Copilot CLI's interactive mode with `--allow-tool 'shell(kube-log)'`, Copilot can call `kube-log` on its own as a shell command. This gives multi-turn drill-down without any MCP:

```
You: The payments service is failing in prod. Investigate.

Copilot: Let me check the pod status.
> kube-log pods --namespace api
  payments-7f8d  CrashLoopBackOff (8 restarts)
  gateway-3a2b   Running (0 restarts)

Copilot: payments is crash-looping. Let me pull the recent error logs.
> kube-log logs --pod payments-7f8d --namespace api --time-range 15m
  { "summary": { "error_count": 23, "top_errors": [...] }, ... }

Copilot: The root cause is database connection failures starting at 10:12 UTC.
         The connection string points to db:5432. Let me check if the gateway
         service shows related errors.
> kube-log logs --pod gateway-3a2b --namespace api --search "database" --time-range 15m

Copilot: Gateway shows no database errors — it uses a different connection pool.
         This is isolated to the payments pod. Likely cause: database failover
         or network policy change at 10:12.
```

This is the same workflow that MCP would enable, but through standard shell piping. The LLM calls `kube-log` as a subprocess, reads stdout, and reasons over the JSON output. No protocol, no server, no config files.

#### OpenCode: Custom Tool (Optional Enhancement)

For teams that want OpenCode to discover `kube-log` as a named tool (rather than calling it via bash), a custom tool definition can ship in the repo:

`.opencode/tools/kube-log.ts`:

```typescript
import { tool } from "@opencode-ai/plugin"

export const diagnose = tool({
  description: "Fetch Kubernetes pod logs with automatic troubleshoot analysis. Suppresses health checks and noise, surfaces errors and anomalies.",
  args: {
    namespace:  tool.schema.string().describe("Kubernetes namespace"),
    pod:        tool.schema.string().describe("Pod name"),
    time_range: tool.schema.string().optional().describe("e.g. '15m', '1h', '6h' (default: 15m)"),
    search:     tool.schema.string().optional().describe("Filter by substring"),
  },
  async execute(args) {
    const flags = [`logs`, `--namespace`, args.namespace, `--pod`, args.pod]
    if (args.time_range) flags.push(`--time-range`, args.time_range)
    if (args.search)     flags.push(`--search`, args.search)
    return await Bun.$`kube-log ${flags}`.text()
  },
})
```

This is nice-to-have, not essential. The pipe does the same job.

#### MCP Server (Future, Not Priority)

An MCP server mode (`kube-log --mcp`) could expose typed tools over stdio JSON-RPC for both OpenCode and Copilot CLI. But this adds significant implementation complexity (MCP protocol in Rust) for marginal benefit over shell piping. Deferred until the ecosystem demands it.

#### Why Not TTY Detection

Following the zig-saju precedent, `kube-log` produces **identical output** regardless of whether stdout is a terminal or pipe. The default format is already LLM-friendly (JSON). There is no "pretty mode for humans, machine mode for pipes" split — the TUI binary is the human interface, the CLI binary is the machine interface. Two binaries, two audiences, no ambiguity.

## Consequences

- The workspace split adds build complexity but enables independent versioning and smaller binary sizes (the CLI doesn't pull in ratatui/crossterm).
- The classify-filter-reduce pipeline adds a processing layer, but it runs on already-fetched data and is bounded by the reduce caps.
- JSON-as-default means the CLI is not human-friendly by default. This is intentional — the TUI is for humans.
- Structural dedup requires maintaining a seen-set of canonical forms per stream. Memory is bounded by capping the set size (e.g., 10K entries with LRU eviction).
- The `--all` escape hatch ensures power users can bypass troubleshoot filtering when they know they need raw output.
- No auth flow in the CLI means users in environments with short-lived tokens must handle refresh externally. This is acceptable — the CLI targets scripted/automated use where auth is managed by the environment.
