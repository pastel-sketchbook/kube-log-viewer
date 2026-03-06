use anyhow::{Context as _, Result};
use futures::AsyncBufReadExt;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, LogParams};
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, info, instrument, warn};

use super::create_client;

/// A single item yielded by a log stream.
#[derive(Debug, Clone)]
pub enum LogStreamItem {
    /// A non-empty log line from the pod.
    Line(String),
    /// A stream-level error (e.g. I/O failure mid-stream).
    Error(String),
}

/// Connect to a pod and return a stream of log items.
///
/// The returned stream yields [`LogStreamItem::Line`] for each log line and
/// [`LogStreamItem::Error`] if the underlying byte stream encounters an error.
/// The stream ends (returns `None`) when the pod terminates, the K8s API
/// closes the connection, or `cancel_rx` fires.
///
/// Setup errors (bad context, unreachable API server, etc.) are returned as
/// `Err(...)` before any stream item is produced.
#[instrument(skip(cancel_rx), fields(context, namespace, pod_name, container))]
pub async fn stream_logs(
    context: &str,
    namespace: &str,
    pod_name: &str,
    container: Option<&str>,
    cancel_rx: watch::Receiver<bool>,
) -> Result<UnboundedReceiverStream<LogStreamItem>> {
    info!("starting log stream");

    let client = create_client(Some(context))
        .await
        .with_context(|| format!("failed to create client for context '{context}'"))?;
    let pod_api: Api<Pod> = Api::namespaced(client, namespace);

    let params = LogParams {
        follow: true,
        tail_lines: Some(100),
        timestamps: true,
        container: container.map(|s| s.to_string()),
        ..Default::default()
    };

    let stream = pod_api
        .log_stream(pod_name, &params)
        .await
        .with_context(|| {
            format!(
                "failed to start log stream for pod '{pod_name}' in namespace '{namespace}' (context '{context}')"
            )
        })?;

    let (tx, rx) = mpsc::unbounded_channel();
    let mut cancel_rx = cancel_rx;

    tokio::spawn(async move {
        let mut lines = stream.lines();
        let mut line_count: u64 = 0;

        loop {
            tokio::select! {
                line = lines.next() => {
                    match line {
                        Some(Ok(text)) => {
                            if !text.is_empty() {
                                line_count += 1;
                                if tx.send(LogStreamItem::Line(text)).is_err() {
                                    // Receiver dropped — stop producing.
                                    break;
                                }
                            }
                        }
                        Some(Err(e)) => {
                            warn!(error = %e, "log stream error");
                            let _ = tx.send(LogStreamItem::Error(format!("Log stream error: {e}")));
                            break;
                        }
                        None => {
                            debug!(line_count, "log stream ended naturally");
                            break;
                        }
                    }
                }
                result = cancel_rx.changed() => {
                    if result.is_err() || *cancel_rx.borrow() {
                        info!(line_count, "log stream cancelled");
                        break;
                    }
                }
            }
        }
    });

    Ok(UnboundedReceiverStream::new(rx))
}
