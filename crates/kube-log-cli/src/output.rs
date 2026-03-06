//! Pipeline execution for CLI subcommands.
//!
//! Wires the K8s data-fetching layer to the classify → filter → reduce →
//! export pipeline. Each subcommand handler fetches data from the cluster,
//! runs it through the appropriate pipeline stages, and writes the result
//! to stdout.

use std::io::{self, Write};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use futures::StreamExt;
use tokio::sync::watch;

use kube_log_core::classify::Classifier;
use kube_log_core::export;
use kube_log_core::filter;
use kube_log_core::k8s;
use kube_log_core::k8s::logs::LogStreamItem;
use kube_log_core::reduce;
use kube_log_core::types::{ClassifiedLine, FilterConfig, PipelineOutput, PodSummary};

use crate::cli::{LogsArgs, NamespacesArgs, PodsArgs};

// ---------------------------------------------------------------------------
// Context / namespace resolution helpers
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

    // Resolve target pods: explicit --pod flags or auto-discover all in namespace.
    let pod_names: Vec<String> = if args.pod.is_empty() {
        let pods = k8s::pods::list_pods(&context, &namespace)
            .await
            .with_context(|| {
                format!("failed to list pods in namespace '{namespace}' (context '{context}')")
            })?;
        if pods.is_empty() {
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
        pods.iter().map(|p| p.name.clone()).collect()
    } else {
        args.pod.clone()
    };

    if args.follow {
        return run_logs_follow(&args, &context, &namespace, &filter_config, &pod_names).await;
    }

    // Batch mode: fetch logs for all pods, classify, filter, reduce, export.
    let mut classifier = Classifier::new();
    let mut all_classified: Vec<ClassifiedLine> = Vec::new();
    let mut pod_summaries: Vec<PodSummary> = Vec::new();

    // Fetch pod info once for summaries.
    let all_pods = k8s::pods::list_pods(&context, &namespace)
        .await
        .with_context(|| {
            format!("failed to list pods in namespace '{namespace}' (context '{context}')")
        })?;

    for pod_name in &pod_names {
        if let Some(info) = all_pods.iter().find(|p| p.name == *pod_name) {
            pod_summaries.push(PodSummary {
                name: info.name.clone(),
                status: info.status.clone(),
                restarts: info.restarts,
            });
        }

        // Stream logs with a cancellation channel.
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let mut stream = k8s::logs::stream_logs(
            &context,
            &namespace,
            pod_name,
            args.container.as_deref(),
            cancel_rx,
        )
        .await
        .with_context(|| format!("failed to start log stream for pod '{pod_name}'"))?;

        let mut line_count: u64 = 0;
        let max_lines = args.lines as u64;

        // Compute the cutoff timestamp for --time-range filtering.
        let cutoff = time_range.map(|dur| Utc::now() - dur);

        while let Some(item) = stream.next().await {
            match item {
                LogStreamItem::Line(text) => {
                    // Apply --search filter.
                    if let Some(ref query) = search
                        && !text.contains(query.as_str())
                    {
                        continue;
                    }

                    let classified =
                        classifier.classify(&text, pod_name, args.container.as_deref());

                    // Apply --time-range filter.
                    if let Some(cutoff_ts) = cutoff
                        && let Some(ts) = classified.timestamp
                        && ts < cutoff_ts
                    {
                        continue;
                    }

                    all_classified.push(classified);
                    line_count += 1;

                    if line_count >= max_lines {
                        break;
                    }
                }
                LogStreamItem::Error(e) => {
                    tracing::warn!(pod = pod_name, error = %e, "log stream error");
                    break;
                }
            }
        }

        // Signal cancellation so the background task cleans up.
        let _ = cancel_tx.send(true);
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
) -> Result<()> {
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    let mut classifier = Classifier::new();
    let search = args.search.clone();
    let time_range = args.parse_time_range()?;
    let cutoff = time_range.map(|dur| Utc::now() - dur);

    // For follow mode we only support a single pod currently.
    // Multi-pod follow would require multiplexing streams.
    if pod_names.len() > 1 {
        bail!(
            "--follow currently supports a single pod (multi-pod follow not yet implemented). Use --pod to select one."
        );
    }
    let pod_name = &pod_names[0];

    let (cancel_tx, cancel_rx) = watch::channel(false);
    let mut stream = k8s::logs::stream_logs(
        context,
        namespace,
        pod_name,
        args.container.as_deref(),
        cancel_rx,
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

                let classified = classifier.classify(&text, pod_name, args.container.as_deref());

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
