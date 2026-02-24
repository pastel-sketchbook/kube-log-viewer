use std::collections::BTreeMap;
use std::pin::pin;

use anyhow::{Context as _, Result};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::api::Api;
use kube::runtime::watcher::{self, Event};
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, instrument};

use super::create_client;
use crate::event::AppEvent;

#[derive(Debug, Clone)]
pub struct PodInfo {
    pub name: String,
    pub status: String,
    pub ready: String,
    pub restarts: i32,
    pub containers: Vec<String>,
}

impl PodInfo {
    /// Extract structured pod info from a raw K8s `Pod` object.
    pub fn from_pod(pod: &Pod) -> Self {
        let name = pod.metadata.name.clone().unwrap_or_default();

        let status = pod
            .status
            .as_ref()
            .and_then(|s| s.phase.clone())
            .unwrap_or_else(|| "Unknown".to_string());

        let containers: Vec<String> = pod
            .spec
            .as_ref()
            .map(|spec| spec.containers.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();

        let (ready_count, total_count, restarts) = pod
            .status
            .as_ref()
            .map(|s| {
                let statuses = s.container_statuses.as_deref().unwrap_or_default();
                let ready = statuses.iter().filter(|cs| cs.ready).count();
                let total = statuses.len();
                let restarts: i32 = statuses.iter().map(|cs| cs.restart_count).sum();
                (ready, total, restarts)
            })
            .unwrap_or((0, containers.len(), 0));

        PodInfo {
            name,
            status,
            ready: format!("{}/{}", ready_count, total_count),
            restarts,
            containers,
        }
    }
}

/// List pods in the given namespace and context, returning structured info.
#[instrument(skip_all, fields(context, namespace))]
pub async fn list_pods(context: &str, namespace: &str) -> Result<Vec<PodInfo>> {
    let client = create_client(Some(context))
        .await
        .with_context(|| format!("failed to create client for context '{context}'"))?;
    let pod_api: Api<Pod> = Api::namespaced(client, namespace);
    let pod_list = pod_api.list(&Default::default()).await.with_context(|| {
        format!("failed to list pods in namespace '{namespace}' (context '{context}')")
    })?;

    let pods: Vec<PodInfo> = pod_list.items.iter().map(PodInfo::from_pod).collect();

    info!(context, namespace, count = pods.len(), "loaded pods");

    Ok(pods)
}

/// Watch pods in the given namespace via the Kubernetes watch API.
///
/// Sends [`AppEvent::PodsUpdated`] whenever the pod list changes.
/// Runs until `cancel_rx` fires or the stream ends.
#[instrument(skip_all, fields(context, namespace))]
pub async fn watch_pods(
    context: &str,
    namespace: &str,
    tx: mpsc::UnboundedSender<AppEvent>,
    mut cancel_rx: watch::Receiver<bool>,
) {
    let client = match create_client(Some(context)).await {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "failed to create client for pod watcher");
            let _ = tx.send(AppEvent::Error(format!("Failed to watch pods: {e:#}")));
            return;
        }
    };

    let api: Api<Pod> = Api::namespaced(client, namespace);
    let stream = pin!(watcher::watcher(api, watcher::Config::default()));
    let mut pods: BTreeMap<String, PodInfo> = BTreeMap::new();
    let mut initialized = false;

    // Stream is fused by the watcher internally; we pin and iterate.
    let mut stream = stream;

    loop {
        tokio::select! {
            result = cancel_rx.changed() => {
                if result.is_err() || *cancel_rx.borrow() {
                    debug!("pod watcher cancelled");
                    break;
                }
            }
            event = stream.next() => {
                match event {
                    Some(Ok(Event::Init)) => {
                        pods.clear();
                        initialized = false;
                    }
                    Some(Ok(Event::InitApply(pod))) => {
                        let info = PodInfo::from_pod(&pod);
                        pods.insert(info.name.clone(), info);
                    }
                    Some(Ok(Event::InitDone)) => {
                        initialized = true;
                        let list: Vec<PodInfo> = pods.values().cloned().collect();
                        info!(count = list.len(), "pod watcher initial sync complete");
                        let _ = tx.send(AppEvent::PodsUpdated(list));
                    }
                    Some(Ok(Event::Apply(pod))) => {
                        let info = PodInfo::from_pod(&pod);
                        pods.insert(info.name.clone(), info);
                        if initialized {
                            let list: Vec<PodInfo> = pods.values().cloned().collect();
                            debug!(count = list.len(), "pod watcher: apply");
                            let _ = tx.send(AppEvent::PodsUpdated(list));
                        }
                    }
                    Some(Ok(Event::Delete(pod))) => {
                        if let Some(name) = pod.metadata.name.as_ref() {
                            pods.remove(name);
                        }
                        if initialized {
                            let list: Vec<PodInfo> = pods.values().cloned().collect();
                            debug!(count = list.len(), "pod watcher: delete");
                            let _ = tx.send(AppEvent::PodsUpdated(list));
                        }
                    }
                    Some(Err(e)) => {
                        // kube-runtime watcher has built-in retry with backoff;
                        // report the error but keep running.
                        error!(error = %e, "pod watcher error");
                        let _ = tx.send(AppEvent::Error(format!("Pod watch error: {e}")));
                    }
                    None => {
                        debug!("pod watcher stream ended");
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{Container, ContainerStatus, PodSpec, PodStatus};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn make_pod(
        name: &str,
        phase: Option<&str>,
        container_names: Vec<&str>,
        container_statuses: Vec<(bool, i32)>, // (ready, restart_count)
    ) -> Pod {
        let spec = if container_names.is_empty() {
            None
        } else {
            Some(PodSpec {
                containers: container_names
                    .iter()
                    .map(|n| Container {
                        name: n.to_string(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            })
        };

        let status = if phase.is_some() || !container_statuses.is_empty() {
            Some(PodStatus {
                phase: phase.map(|p| p.to_string()),
                container_statuses: if container_statuses.is_empty() {
                    None
                } else {
                    Some(
                        container_statuses
                            .iter()
                            .enumerate()
                            .map(|(i, (ready, restarts))| ContainerStatus {
                                name: container_names.get(i).unwrap_or(&"unknown").to_string(),
                                ready: *ready,
                                restart_count: *restarts,
                                ..Default::default()
                            })
                            .collect(),
                    )
                },
                ..Default::default()
            })
        } else {
            None
        };

        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                ..Default::default()
            },
            spec,
            status,
        }
    }

    #[test]
    fn test_running_pod_with_two_ready_containers() {
        let pod = make_pod(
            "nginx-abc123",
            Some("Running"),
            vec!["nginx", "sidecar"],
            vec![(true, 0), (true, 3)],
        );
        let info = PodInfo::from_pod(&pod);

        assert_eq!(info.name, "nginx-abc123");
        assert_eq!(info.status, "Running");
        assert_eq!(info.ready, "2/2");
        assert_eq!(info.restarts, 3);
        assert_eq!(info.containers, vec!["nginx", "sidecar"]);
    }

    #[test]
    fn test_pending_pod_with_no_ready_containers() {
        let pod = make_pod("app-xyz789", Some("Pending"), vec!["app"], vec![(false, 0)]);
        let info = PodInfo::from_pod(&pod);

        assert_eq!(info.name, "app-xyz789");
        assert_eq!(info.status, "Pending");
        assert_eq!(info.ready, "0/1");
        assert_eq!(info.restarts, 0);
        assert_eq!(info.containers, vec!["app"]);
    }

    #[test]
    fn test_pod_with_missing_status() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("ghost-pod".to_string()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "ghost".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            status: None,
        };
        let info = PodInfo::from_pod(&pod);

        assert_eq!(info.name, "ghost-pod");
        assert_eq!(info.status, "Unknown");
        // No status => ready defaults to 0/container_count
        assert_eq!(info.ready, "0/1");
        assert_eq!(info.restarts, 0);
    }

    #[test]
    fn test_pod_with_missing_name() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: None,
                ..Default::default()
            },
            spec: None,
            status: None,
        };
        let info = PodInfo::from_pod(&pod);

        assert_eq!(info.name, "");
        assert_eq!(info.status, "Unknown");
        assert_eq!(info.ready, "0/0");
        assert_eq!(info.restarts, 0);
        assert!(info.containers.is_empty());
    }

    #[test]
    fn test_pod_partial_readiness() {
        let pod = make_pod(
            "mixed-pod",
            Some("Running"),
            vec!["web", "worker", "sidecar"],
            vec![(true, 1), (false, 5), (true, 0)],
        );
        let info = PodInfo::from_pod(&pod);

        assert_eq!(info.ready, "2/3");
        assert_eq!(info.restarts, 6);
        assert_eq!(info.containers.len(), 3);
    }

    #[test]
    fn test_failed_pod() {
        let pod = make_pod("crash-pod", Some("Failed"), vec!["app"], vec![(false, 42)]);
        let info = PodInfo::from_pod(&pod);

        assert_eq!(info.status, "Failed");
        assert_eq!(info.ready, "0/1");
        assert_eq!(info.restarts, 42);
    }
}
