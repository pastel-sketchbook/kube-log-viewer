use anyhow::{Context as _, Result};
use k8s_openapi::api::core::v1::Namespace;
use kube::api::Api;
use tracing::{info, instrument};

use super::create_client;

/// List all namespace names in the cluster for the given context.
#[instrument(skip_all, fields(context))]
pub async fn list_namespaces(context: &str) -> Result<Vec<String>> {
    let client = create_client(Some(context))
        .await
        .with_context(|| format!("failed to create client for context '{context}'"))?;
    let ns_api: Api<Namespace> = Api::all(client);
    let ns_list = ns_api
        .list(&Default::default())
        .await
        .with_context(|| format!("failed to list namespaces in context '{context}'"))?;

    let mut namespaces: Vec<String> = ns_list
        .items
        .iter()
        .filter_map(|ns| ns.metadata.name.clone())
        .collect();
    namespaces.sort();

    info!(context, count = namespaces.len(), "loaded namespaces");

    Ok(namespaces)
}
