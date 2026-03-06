//! Pipeline execution for CLI subcommands.
//!
//! Wires the K8s data-fetching layer to the classify → filter → reduce →
//! export pipeline. Each subcommand handler fetches data from the cluster,
//! runs it through the appropriate pipeline stages, and writes the result
//! to stdout.

use std::io::{self, Write};

use anyhow::{Context, Result, bail};
use futures::StreamExt;
use jiff::Timestamp;
use tokio::sync::watch;

use kube_log_core::classify::Classifier;
use kube_log_core::export;
use kube_log_core::filter;
use kube_log_core::k8s;
use kube_log_core::k8s::logs::{LogStreamConfig, LogStreamItem};
use kube_log_core::reduce;
use kube_log_core::types::{ClassifiedLine, FilterConfig, PipelineOutput, PodSummary};

use kube_log_core::k8s::pods::PodInfo;

use crate::cli::{LogsArgs, NamespacesArgs, PodsArgs};

// ---------------------------------------------------------------------------
// Well-known sidecar / init container names
// ---------------------------------------------------------------------------

/// Container names that are typically injected by service meshes, secret
/// managers, or platform tooling.  When a pod has multiple containers and the
/// user did not specify `--container`, we filter these out so the CLI
/// auto-selects the application container.
const KNOWN_SIDECARS: &[&str] = &[
    // Istio
    "istio-init",
    "istio-proxy",
    // Envoy standalone
    "envoy",
    "envoy-proxy",
    // Linkerd
    "linkerd-init",
    "linkerd-proxy",
    // Vault agent injector
    "vault-agent",
    "vault-agent-init",
    // AWS App Mesh
    "appmesh-envoy",
    // Datadog
    "datadog-agent",
    // Fluentd / Fluent Bit sidecar logging
    "fluentd",
    "fluent-bit",
    // CloudSQL proxy (GCP)
    "cloud-sql-proxy",
    "cloudsql-proxy",
];

// ---------------------------------------------------------------------------
// Context / namespace / container resolution helpers
// ---------------------------------------------------------------------------

/// Resolve the K8s context to use: explicit `--context` flag or current from kubeconfig.
fn resolve_context(explicit: &Option<String>) -> Result<String> {
    if let Some(ctx) = explicit {
        return Ok(ctx.clone());
    }
    let (_contexts, current) = k8s::contexts::load_contexts()
        .context("failed to load kubeconfig to determine current context")?;
    if current.is_empty() {
        bail!("no current context in kubeconfig and --context not specified");
    }
    Ok(current)
}

/// Resolve the namespace: explicit `--namespace` flag or fall back to "default".
fn resolve_namespace(explicit: &Option<String>) -> String {
    explicit.clone().unwrap_or_else(|| "default".to_string())
}

/// Determine which container(s) to stream logs from.
///
/// - If the user specified `--container`, return that single container.
/// - If the pod has exactly one container, return it.
/// - If the pod has multiple containers, filter out well-known sidecars/init
///   containers and return the remaining "application" containers.
/// - If filtering removes *all* containers (unlikely but defensive), fall back
///   to the full list so we never return an empty set.
fn resolve_containers(explicit: &Option<String>, pod_info: Option<&PodInfo>) -> Vec<String> {
    // Explicit --container always wins.
    if let Some(c) = explicit {
        return vec![c.clone()];
    }

    let containers = match pod_info {
        Some(info) if !info.containers.is_empty() => &info.containers,
        // No info available — return empty so the caller passes `None`
        // (single-container pods work fine with container=None).
        _ => return Vec::new(),
    };

    if containers.len() == 1 {
        return containers.clone();
    }

    // Multiple containers: filter out known sidecars.
    let filtered: Vec<String> = containers
        .iter()
        .filter(|name| !KNOWN_SIDECARS.contains(&name.as_str()))
        .cloned()
        .collect();

    // If filtering removed everything, use the full list.
    if filtered.is_empty() {
        containers.clone()
    } else {
        filtered
    }
}

// ---------------------------------------------------------------------------
// logs subcommand
// ---------------------------------------------------------------------------

/// Suggest namespaces similar to `input` by substring matching.
///
/// Returns namespaces where either the input is a substring of the namespace
/// or the namespace is a substring of the input (covers prefix/suffix typos).
async fn suggest_namespaces(context: &str, input: &str) -> Vec<String> {
    let Ok(all_ns) = k8s::namespaces::list_namespaces(context).await else {
        return Vec::new();
    };
    let lower = input.to_lowercase();
    all_ns
        .into_iter()
        .filter(|ns| {
            let ns_lower = ns.to_lowercase();
            ns_lower.contains(&lower) || lower.contains(&ns_lower)
        })
        .collect()
}

/// Execute the `logs` subcommand: fetch → classify → filter → reduce → export.
pub async fn run_logs(args: LogsArgs) -> Result<()> {
    let context = resolve_context(&args.context)?;
    let namespace = resolve_namespace(&args.namespace);
    let filter_config = args.filter_config();
    let output_format = args.output_format();
    let time_range = args.parse_time_range()?;
    let search = args.search.clone();

    // Fetch pod info once — used for pod discovery, container resolution, and summaries.
    let all_pods = k8s::pods::list_pods(&context, &namespace)
        .await
        .with_context(|| {
            format!("failed to list pods in namespace '{namespace}' (context '{context}')")
        })?;

    // Resolve target pods: explicit --pod flags or auto-discover all in namespace.
    let pod_names: Vec<String> = if args.pod.is_empty() {
        if all_pods.is_empty() {
            let suggestions = suggest_namespaces(&context, &namespace).await;
            if suggestions.is_empty() {
                bail!("no pods found in namespace '{namespace}' (context '{context}')");
            } else {
                bail!(
                    "no pods found in namespace '{namespace}' (context '{context}'). Did you mean: {}",
                    suggestions.join(", ")
                );
            }
        }
        all_pods.iter().map(|p| p.name.clone()).collect()
    } else {
        args.pod.clone()
    };

    if args.follow {
        return run_logs_follow(
            &args,
            &context,
            &namespace,
            &filter_config,
            &pod_names,
            &all_pods,
        )
        .await;
    }

    // Batch mode: fetch logs for all pods, classify, filter, reduce, export.
    let mut classifier = Classifier::new();
    let mut all_classified: Vec<ClassifiedLine> = Vec::new();
    let mut pod_summaries: Vec<PodSummary> = Vec::new();

    for pod_name in &pod_names {
        let pod_info = all_pods.iter().find(|p| p.name == *pod_name);

        if let Some(info) = pod_info {
            pod_summaries.push(PodSummary {
                name: info.name.clone(),
                status: info.status.clone(),
                restarts: info.restarts,
            });
        }

        // Resolve which container(s) to stream logs from.
        let containers = resolve_containers(&args.container, pod_info);

        // If resolve_containers returned an empty list, the pod likely has a
        // single container and the K8s API can infer it — stream once with
        // container=None. Otherwise stream each resolved container.
        let container_targets: Vec<Option<&str>> = if containers.is_empty() {
            vec![None]
        } else {
            containers.iter().map(|c| Some(c.as_str())).collect()
        };

        for container in &container_targets {
            // Build the stream config for batch mode: no follow, server-side
            // tail/time filtering so we don't download the full log history.
            let stream_config = LogStreamConfig {
                follow: false,
                tail_lines: args.effective_tail_lines(),
                since_seconds: time_range.map(|dur| dur.as_secs()),
                timestamps: true,
            };

            // Stream logs with a cancellation channel.
            let (cancel_tx, cancel_rx) = watch::channel(false);
            let mut stream = k8s::logs::stream_logs(
                &context,
                &namespace,
                pod_name,
                *container,
                cancel_rx,
                &stream_config,
            )
            .await
            .with_context(|| match container {
                Some(c) => {
                    format!("failed to start log stream for pod '{pod_name}' container '{c}'")
                }
                None => format!("failed to start log stream for pod '{pod_name}'"),
            })?;

            let mut line_count: u64 = 0;
            let max_lines = args.effective_max_lines();

            // Compute the cutoff timestamp for --time-range filtering.
            let cutoff = time_range.map(|dur| Timestamp::now() - dur);

            while let Some(item) = stream.next().await {
                match item {
                    LogStreamItem::Line(text) => {
                        // Apply --search filter.
                        if let Some(ref query) = search
                            && !text.contains(query.as_str())
                        {
                            continue;
                        }

                        let classified = classifier.classify(&text, pod_name, *container);

                        // Apply --time-range filter.
                        if let Some(cutoff_ts) = cutoff
                            && let Some(ts) = classified.timestamp
                            && ts < cutoff_ts
                        {
                            continue;
                        }

                        all_classified.push(classified);
                        line_count += 1;

                        if let Some(cap) = max_lines
                            && line_count >= cap
                        {
                            break;
                        }
                    }
                    LogStreamItem::Error(e) => {
                        tracing::warn!(
                            pod = pod_name,
                            container = *container,
                            error = %e,
                            "log stream error"
                        );
                        break;
                    }
                }
            }

            // Signal cancellation so the background task cleans up.
            let _ = cancel_tx.send(true);
        }
    }

    // Run the filter stage.
    let (filtered_items, filter_stats) = filter::filter(all_classified.clone(), &filter_config);

    // Collect kept ClassifiedLines for the reduce stage and output.
    let kept_lines: Vec<ClassifiedLine> = filtered_items
        .iter()
        .filter_map(|item| {
            if let filter::FilteredItem::Line(line) = item {
                Some(line.clone())
            } else {
                None
            }
        })
        .collect();

    // Run the reduce stage.
    let mut summary = reduce::reduce(&all_classified, &filter_stats);
    summary.pods = pod_summaries;

    // Build the pipeline output.
    let hint = export::generate_hint(&context, &namespace, &summary);
    let pipeline_output = PipelineOutput {
        hint,
        summary,
        lines: kept_lines,
    };

    // Export to stdout.
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    export::export(
        &mut writer,
        output_format,
        &pipeline_output,
        &filtered_items,
    )
    .context("failed to write output")?;
    writer.flush().context("failed to flush stdout")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Follow mode (streaming)
// ---------------------------------------------------------------------------

/// Stream logs continuously in JSON-lines format.
///
/// In follow mode, each line is classified and immediately written to stdout.
/// No reduce/summary stage — the consumer handles aggregation.
async fn run_logs_follow(
    args: &LogsArgs,
    context: &str,
    namespace: &str,
    filter_config: &FilterConfig,
    pod_names: &[String],
    all_pods: &[PodInfo],
) -> Result<()> {
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    let mut classifier = Classifier::new();
    let search = args.search.clone();
    let time_range = args.parse_time_range()?;
    let cutoff = time_range.map(|dur| Timestamp::now() - dur);

    // For follow mode we only support a single pod currently.
    // Multi-pod follow would require multiplexing streams.
    if pod_names.len() > 1 {
        bail!(
            "--follow currently supports a single pod (multi-pod follow not yet implemented). Use --pod to select one."
        );
    }
    let pod_name = &pod_names[0];

    // Resolve which container(s) to follow.
    let pod_info = all_pods.iter().find(|p| p.name == *pod_name);
    let containers = resolve_containers(&args.container, pod_info);

    // For follow mode with multiple containers we'd need to multiplex streams.
    // For now, if there are multiple application containers, pick the first one
    // and warn.  The user can always specify --container explicitly.
    let container: Option<&str> = if containers.is_empty() {
        None
    } else {
        if containers.len() > 1 {
            eprintln!(
                "warning: pod '{pod_name}' has {} application containers ({}); following '{}'. Use --container to select a different one.",
                containers.len(),
                containers.join(", "),
                containers[0],
            );
        }
        Some(containers[0].as_str())
    };

    let follow_config = LogStreamConfig {
        follow: true,
        tail_lines: args.effective_tail_lines(),
        since_seconds: time_range.map(|dur| dur.as_secs()),
        timestamps: true,
    };

    let (cancel_tx, cancel_rx) = watch::channel(false);
    let mut stream = k8s::logs::stream_logs(
        context,
        namespace,
        pod_name,
        container,
        cancel_rx,
        &follow_config,
    )
    .await
    .with_context(|| format!("failed to start log stream for pod '{pod_name}'"))?;

    // Install ctrl-c handler to cancel the stream gracefully.
    let cancel_tx_clone = cancel_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install ctrl-c handler");
        let _ = cancel_tx_clone.send(true);
    });

    while let Some(item) = stream.next().await {
        match item {
            LogStreamItem::Line(text) => {
                // Apply --search filter.
                if let Some(ref query) = search
                    && !text.contains(query.as_str())
                {
                    continue;
                }

                let classified = classifier.classify(&text, pod_name, container);

                // Apply --time-range filter.
                if let Some(cutoff_ts) = cutoff
                    && let Some(ts) = classified.timestamp
                    && ts < cutoff_ts
                {
                    continue;
                }

                // Apply class filter.
                if !filter_config.should_include(&classified.class) {
                    continue;
                }

                // Write as JSONL (one JSON object per line for streaming).
                let json = serde_json::to_string(&classified)
                    .context("failed to serialize classified line")?;
                writeln!(writer, "{json}").context("failed to write to stdout")?;
                writer.flush().context("failed to flush stdout")?;
            }
            LogStreamItem::Error(e) => {
                tracing::warn!(pod = pod_name, error = %e, "log stream error");
                let error_json = serde_json::json!({
                    "error": "stream_error",
                    "message": e,
                    "pod": pod_name,
                });
                writeln!(
                    writer,
                    "{}",
                    serde_json::to_string(&error_json).unwrap_or_default()
                )
                .context("failed to write error to stdout")?;
                writer.flush().context("failed to flush stdout")?;
            }
        }
    }

    // Ensure cancellation fires on clean exit.
    let _ = cancel_tx.send(true);

    Ok(())
}

// ---------------------------------------------------------------------------
// pods subcommand
// ---------------------------------------------------------------------------

/// Execute the `pods` subcommand: list pods as JSON.
pub async fn run_pods(args: PodsArgs) -> Result<()> {
    let context = resolve_context(&args.context)?;
    let namespace = resolve_namespace(&args.namespace);

    let pods = k8s::pods::list_pods(&context, &namespace)
        .await
        .with_context(|| {
            format!("failed to list pods in namespace '{namespace}' (context '{context}')")
        })?;

    let output: Vec<serde_json::Value> = pods
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "status": p.status,
                "ready": p.ready,
                "restarts": p.restarts,
                "containers": p.containers,
            })
        })
        .collect();

    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    let json = serde_json::to_string_pretty(&output).context("failed to serialize pod list")?;
    writeln!(writer, "{json}").context("failed to write pod list")?;
    writer.flush().context("failed to flush stdout")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// contexts subcommand
// ---------------------------------------------------------------------------

/// Execute the `contexts` subcommand: list K8s contexts.
pub async fn run_contexts() -> Result<()> {
    let (contexts, current) =
        k8s::contexts::load_contexts().context("failed to load kubeconfig contexts")?;

    let output = serde_json::json!({
        "current": current,
        "contexts": contexts,
    });

    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    let json = serde_json::to_string_pretty(&output).context("failed to serialize contexts")?;
    writeln!(writer, "{json}").context("failed to write contexts")?;
    writer.flush().context("failed to flush stdout")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// namespaces subcommand
// ---------------------------------------------------------------------------

/// Execute the `namespaces` subcommand: list namespaces.
pub async fn run_namespaces(args: NamespacesArgs) -> Result<()> {
    let context = resolve_context(&args.context)?;

    let namespaces = k8s::namespaces::list_namespaces(&context)
        .await
        .with_context(|| format!("failed to list namespaces for context '{context}'"))?;

    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    let json =
        serde_json::to_string_pretty(&namespaces).context("failed to serialize namespaces")?;
    writeln!(writer, "{json}").context("failed to write namespaces")?;
    writer.flush().context("failed to flush stdout")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use kube_log_core::k8s::pods::PodInfo;

    /// Helper to build a `PodInfo` with the given container names.
    fn pod_with_containers(name: &str, containers: Vec<&str>) -> PodInfo {
        PodInfo {
            name: name.to_string(),
            status: "Running".to_string(),
            ready: format!("{}/{}", containers.len(), containers.len()),
            restarts: 0,
            containers: containers.into_iter().map(String::from).collect(),
        }
    }

    // -- resolve_containers --------------------------------------------------

    #[test]
    fn explicit_container_wins_over_pod_info() {
        let pod = pod_with_containers("my-pod", vec!["app", "istio-proxy"]);
        let result = resolve_containers(&Some("istio-proxy".to_string()), Some(&pod));
        assert_eq!(result, vec!["istio-proxy"]);
    }

    #[test]
    fn single_container_returns_it() {
        let pod = pod_with_containers("my-pod", vec!["app"]);
        let result = resolve_containers(&None, Some(&pod));
        assert_eq!(result, vec!["app"]);
    }

    #[test]
    fn multi_container_filters_sidecars() {
        let pod = pod_with_containers("my-pod", vec!["istio-init", "my-app", "istio-proxy"]);
        let result = resolve_containers(&None, Some(&pod));
        assert_eq!(result, vec!["my-app"]);
    }

    #[test]
    fn multi_container_keeps_multiple_app_containers() {
        let pod = pod_with_containers("my-pod", vec!["istio-init", "web", "worker", "istio-proxy"]);
        let result = resolve_containers(&None, Some(&pod));
        assert_eq!(result, vec!["web", "worker"]);
    }

    #[test]
    fn all_sidecar_pod_returns_full_list() {
        // Edge case: every container is a known sidecar (unlikely but defensive).
        let pod = pod_with_containers(
            "sidecar-only",
            vec!["istio-init", "istio-proxy", "vault-agent"],
        );
        let result = resolve_containers(&None, Some(&pod));
        // Filtering removed everything → fall back to full list.
        assert_eq!(result, vec!["istio-init", "istio-proxy", "vault-agent"]);
    }

    #[test]
    fn no_pod_info_returns_empty() {
        let result = resolve_containers(&None, None);
        assert!(result.is_empty());
    }

    #[test]
    fn empty_containers_returns_empty() {
        let pod = pod_with_containers("empty-pod", vec![]);
        let result = resolve_containers(&None, Some(&pod));
        assert!(result.is_empty());
    }

    #[test]
    fn linkerd_sidecars_are_filtered() {
        let pod = pod_with_containers(
            "my-pod",
            vec!["linkerd-init", "api-server", "linkerd-proxy"],
        );
        let result = resolve_containers(&None, Some(&pod));
        assert_eq!(result, vec!["api-server"]);
    }

    #[test]
    fn vault_agent_sidecars_are_filtered() {
        let pod = pod_with_containers("my-pod", vec!["vault-agent-init", "backend", "vault-agent"]);
        let result = resolve_containers(&None, Some(&pod));
        assert_eq!(result, vec!["backend"]);
    }

    #[test]
    fn cloud_sql_proxy_is_filtered() {
        let pod = pod_with_containers("my-pod", vec!["cloud-sql-proxy", "django-app"]);
        let result = resolve_containers(&None, Some(&pod));
        assert_eq!(result, vec!["django-app"]);
    }

    // -- KNOWN_SIDECARS constant ---------------------------------------------

    #[test]
    fn known_sidecars_has_expected_entries() {
        // Smoke test: ensure key sidecar names are present.
        assert!(KNOWN_SIDECARS.contains(&"istio-init"));
        assert!(KNOWN_SIDECARS.contains(&"istio-proxy"));
        assert!(KNOWN_SIDECARS.contains(&"envoy"));
        assert!(KNOWN_SIDECARS.contains(&"linkerd-proxy"));
        assert!(KNOWN_SIDECARS.contains(&"vault-agent"));
        assert!(KNOWN_SIDECARS.contains(&"cloud-sql-proxy"));
        assert!(KNOWN_SIDECARS.contains(&"datadog-agent"));
    }
}
