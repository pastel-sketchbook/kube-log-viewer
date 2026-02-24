use anyhow::{Context, Result};
use kube::config::Kubeconfig;

/// Load the list of contexts and the current context from `~/.kube/config`.
pub fn load_contexts() -> Result<(Vec<String>, String)> {
    let kubeconfig =
        Kubeconfig::read().context("failed to read kubeconfig -- is ~/.kube/config present?")?;

    let contexts: Vec<String> = kubeconfig.contexts.iter().map(|c| c.name.clone()).collect();

    let current = kubeconfig.current_context.unwrap_or_default();

    Ok((contexts, current))
}
