use anyhow::{Context, Result};
use kube::config::Kubeconfig;
use tracing::{debug, info, instrument};

/// Load the list of contexts and the current context from `~/.kube/config`.
#[instrument]
pub fn load_contexts() -> Result<(Vec<String>, String)> {
    let kubeconfig =
        Kubeconfig::read().context("failed to read kubeconfig -- is ~/.kube/config present?")?;

    let contexts: Vec<String> = kubeconfig.contexts.iter().map(|c| c.name.clone()).collect();

    let current = kubeconfig.current_context.unwrap_or_default();

    info!(count = contexts.len(), current = %current, "loaded k8s contexts");
    debug!(?contexts, "available contexts");

    Ok((contexts, current))
}
