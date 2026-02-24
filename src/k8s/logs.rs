use anyhow::{Context as _, Result};
use futures::AsyncBufReadExt;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, LogParams};
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, instrument, warn};

use crate::event::AppEvent;

use super::create_client;

/// Stream log lines from a pod, sending each line over `tx`.
///
/// Respects the `cancel_rx` watch channel for cooperative cancellation.
#[instrument(skip(cancel_rx, tx), fields(context, namespace, pod_name, container))]
pub async fn stream_logs(
    context: &str,
    namespace: &str,
    pod_name: &str,
    container: Option<&str>,
    mut cancel_rx: watch::Receiver<bool>,
    tx: mpsc::UnboundedSender<AppEvent>,
) -> Result<()> {
    info!("starting log stream");

    let client = create_client(Some(context)).await?;
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
        .with_context(|| format!("failed to start log stream for pod '{pod_name}'"))?;

    let mut lines = stream.lines();
    let mut line_count: u64 = 0;

    loop {
        tokio::select! {
            line = lines.next() => {
                match line {
                    Some(Ok(text)) => {
                        if !text.is_empty() {
                            line_count += 1;
                            let _ = tx.send(AppEvent::LogLine(pod_name.to_string(), text));
                        }
                    }
                    Some(Err(e)) => {
                        warn!(error = %e, "log stream error");
                        let _ = tx.send(AppEvent::Error(format!("Log stream error: {e}")));
                        break;
                    }
                    None => {
                        debug!(line_count, "log stream ended naturally");
                        break;
                    }
                }
            }
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() {
                    info!(line_count, "log stream cancelled");
                    break;
                }
            }
        }
    }

    Ok(())
}
