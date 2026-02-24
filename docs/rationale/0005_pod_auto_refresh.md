# ADR-0005: Pod Auto-Refresh via Kubernetes Watch

**Date:** 2026-02-24
**Status:** Accepted

## Context

The pod list is currently a point-in-time snapshot fetched via `Api::list()`. Once loaded, it never updates until the user manually switches namespace or context. This means:

1. **New deployments invisible.** A rollout creating new pods won't appear until the user re-triggers a load.
2. **Stale status.** A pod transitioning from `Pending` to `Running` (or crashing) stays frozen at its initial status.
3. **Deleted pods linger.** Terminated pods remain in the list indefinitely.

Operators expect the pod list to reflect reality in near-real-time, like `kubectl get pods --watch`.

## Decision

Replace the one-shot `load_pods()` with a persistent **`kube::runtime::watcher`** background task that maintains a live pod list via the Kubernetes watch API.

### Architecture

```
start_pod_watcher()
        │
        ▼
  tokio::spawn ─────────────────────────────────────────┐
  │                                                      │
  │  create_client(context)                              │
  │  Api::<Pod>::namespaced(client, namespace)            │
  │  watcher(api, Config::default())                     │
  │                                                      │
  │  ┌─ loop ──────────────────────────────────────────┐ │
  │  │ tokio::select! {                                │ │
  │  │   cancel_rx.changed() => break,                 │ │
  │  │   event = stream.next() => {                    │ │
  │  │     Init        → pods.clear(), initialized=f   │ │
  │  │     InitApply   → pods.insert(...)              │ │
  │  │     InitDone    → initialized=true, send update │ │
  │  │     Apply       → pods.insert(...), send if init│ │
  │  │     Delete      → pods.remove(...), send if init│ │
  │  │     Err         → send Error (watcher retries)  │ │
  │  │   }                                             │ │
  │  │ }                                               │ │
  │  └─────────────────────────────────────────────────┘ │
  └──────────────────────────────────────────────────────┘
```

**Key behaviors:**

- **Initial sync**: `Init` → N × `InitApply` → `InitDone`. The full pod list is sent only on `InitDone` to avoid N intermediate updates.
- **Incremental updates**: After `InitDone`, each `Apply` or `Delete` sends the updated list immediately.
- **Cancellation**: A `watch::channel<bool>` cancels the watcher on namespace switch, context switch, or app exit.
- **Error resilience**: `kube::runtime::watcher` has built-in retry with exponential backoff. Transient errors are reported via `AppEvent::Error` but don't kill the watcher.

### Selection preservation

When `PodsUpdated` arrives, the handler must preserve the currently selected pod **by name**, not by index. The list may be reordered or have items inserted/removed. If the previously selected pod no longer exists, fall back to index 0.

### Crate changes

Add the `runtime` feature to the `kube` dependency:

```toml
kube = { version = "3.0.1", features = ["client", "config", "runtime", "rustls-tls"] }
```

This pulls in `kube-runtime` which provides `watcher`, `watcher::Event`, and `watcher::Config`.

## Consequences

- **Positive**: Pod list stays current without user intervention. Deployments, scaling events, and pod crashes are reflected immediately.
- **Positive**: Eliminates the need for manual refresh or polling timers.
- **Positive**: Fits existing channel-based architecture — the watcher task sends `PodsUpdated` events like before.
- **Negative**: Adds a persistent watch connection per namespace. Acceptable for a single-namespace viewer.
- **Negative**: Adds `kube-runtime` as a transitive dependency (already in the kube ecosystem, minimal size impact).
