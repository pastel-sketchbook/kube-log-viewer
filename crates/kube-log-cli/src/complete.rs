//! Dynamic shell completion functions for K8s resources.
//!
//! Each completer queries live cluster state (or local kubeconfig) to provide
//! tab-completion candidates. These are wired into clap args via
//! `ArgValueCompleter`.

use std::ffi::OsStr;

use clap_complete::engine::CompletionCandidate;

/// Complete `--context` values from `~/.kube/config` (local file, no network).
pub fn context_completer(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok((contexts, _current)) = kube_log_core::k8s::contexts::load_contexts() else {
        return vec![];
    };
    let prefix = current.to_string_lossy();
    contexts
        .into_iter()
        .filter(|c| c.starts_with(prefix.as_ref()))
        .map(CompletionCandidate::new)
        .collect()
}

/// Complete `--namespace` values by querying the K8s API.
///
/// Uses the current kubeconfig context (we don't have access to the
/// already-parsed `--context` arg from this completer, so we use the default).
pub fn namespace_completer(current: &OsStr) -> Vec<CompletionCandidate> {
    // Build a small tokio runtime for the async K8s API call.
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return vec![];
    };

    let context = resolve_context_for_completion();
    let Ok(namespaces) = rt.block_on(kube_log_core::k8s::namespaces::list_namespaces(&context))
    else {
        return vec![];
    };

    let prefix = current.to_string_lossy();
    namespaces
        .into_iter()
        .filter(|ns| ns.starts_with(prefix.as_ref()))
        .map(CompletionCandidate::new)
        .collect()
}

/// Complete `--pod` values by querying the K8s API.
///
/// Uses current context and "default" namespace (we can't access parsed args
/// from the completer, so we fall back to defaults).
pub fn pod_completer(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return vec![];
    };

    let context = resolve_context_for_completion();
    // Fall back to "default" namespace since we can't read --namespace here.
    let namespace = "default".to_string();

    let Ok(pods) = rt.block_on(kube_log_core::k8s::pods::list_pods(&context, &namespace)) else {
        return vec![];
    };

    let prefix = current.to_string_lossy();
    pods.into_iter()
        .filter(|p| p.name.starts_with(prefix.as_ref()))
        .map(|p| {
            let help = format!("{} ({} restarts)", p.status, p.restarts);
            CompletionCandidate::new(p.name).help(Some(help.into()))
        })
        .collect()
}

/// Resolve the current K8s context from kubeconfig for completion.
fn resolve_context_for_completion() -> String {
    kube_log_core::k8s::contexts::load_contexts()
        .map(|(_, current)| current)
        .unwrap_or_default()
}
