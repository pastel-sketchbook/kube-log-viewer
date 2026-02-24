use anyhow::{Context as _, Result};
use k8s_openapi::api::core::v1::Pod;
use kube::api::Api;

use super::create_client;

#[derive(Debug, Clone)]
pub struct PodInfo {
    pub name: String,
    pub status: String,
    pub ready: String,
    pub restarts: i32,
    pub containers: Vec<String>,
}

/// List pods in the given namespace and context, returning structured info.
pub async fn list_pods(context: &str, namespace: &str) -> Result<Vec<PodInfo>> {
    let client = create_client(Some(context)).await?;
    let pod_api: Api<Pod> = Api::namespaced(client, namespace);
    let pod_list = pod_api
        .list(&Default::default())
        .await
        .with_context(|| format!("failed to list pods in namespace '{namespace}'"))?;

    let pods: Vec<PodInfo> = pod_list
        .items
        .iter()
        .map(|pod| {
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
        })
        .collect();

    Ok(pods)
}
