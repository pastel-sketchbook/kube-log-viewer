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

    filter_namespaces(&namespaces, current)
}

/// Filter namespace names by prefix and convert to [`CompletionCandidate`]s.
fn filter_namespaces(namespaces: &[String], current: &OsStr) -> Vec<CompletionCandidate> {
    let prefix = current.to_string_lossy();
    namespaces
        .iter()
        .filter(|ns| ns.starts_with(prefix.as_ref()))
        .map(|ns| CompletionCandidate::new(ns.as_str()))
        .collect()
}

/// Complete `--pod` values by querying the K8s API.
///
/// Scans `std::env::args()` for an already-typed `--namespace`/`-n` flag so
/// that `kube-log logs -n kube-system --pod <TAB>` completes pods from the
/// correct namespace. Falls back to "default" if the flag is absent.
pub fn pod_completer(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return vec![];
    };

    let context = resolve_context_for_completion();
    let namespace = resolve_namespace_for_completion();

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
    // Check if the user already typed --context on the command line.
    if let Some(ctx) = extract_flag_value_from_args(&["--context"]) {
        return ctx;
    }
    kube_log_core::k8s::contexts::load_contexts()
        .map(|(_, current)| current)
        .unwrap_or_default()
}

/// Resolve the namespace for completion by scanning already-typed args.
///
/// Looks for `--namespace <value>`, `--namespace=<value>`, `-n <value>`, or
/// `-n<value>` (short flag without space) in `std::env::args()`. Falls back
/// to `"default"` if absent.
fn resolve_namespace_for_completion() -> String {
    extract_flag_value_from_args(&["--namespace", "-n"]).unwrap_or_else(|| "default".to_string())
}

/// Extract the value of a flag from `std::env::args()`.
///
/// Supports:
/// - `--flag value` / `-f value` (space-separated)
/// - `--flag=value` (equals-separated, long flags only)
/// - `-fvalue` (short flag without space, only for single-char short flags)
///
/// Returns the first match found. `flag_names` should list all aliases
/// (e.g. `&["--namespace", "-n"]`).
fn extract_flag_value_from_args(flag_names: &[&str]) -> Option<String> {
    extract_flag_value(&std::env::args().collect::<Vec<_>>(), flag_names)
}

/// Core logic for extracting a flag value from an argument list.
///
/// Separated from [`extract_flag_value_from_args`] so unit tests can pass
/// an explicit slice instead of relying on `std::env::args()`.
fn extract_flag_value(args: &[String], flag_names: &[&str]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        for &flag in flag_names {
            // --flag=value
            if flag.starts_with("--")
                && let Some(val) = arg
                    .strip_prefix(flag)
                    .and_then(|rest| rest.strip_prefix('='))
                && !val.is_empty()
            {
                return Some(val.to_string());
            }

            // Exact match: --flag value / -f value
            if arg == flag
                && let Some(val) = args.get(i + 1)
                && !val.starts_with('-')
            {
                return Some(val.clone());
            }

            // -fvalue (short flag glued to value, e.g. -nkube-system)
            if flag.starts_with('-')
                && !flag.starts_with("--")
                && flag.len() == 2
                && arg.starts_with(flag)
                && arg.len() > flag.len()
            {
                return Some(arg[flag.len()..].to_string());
            }
        }

        i += 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate_values(candidates: &[CompletionCandidate]) -> Vec<String> {
        candidates
            .iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn filter_namespaces_matches_prefix() {
        let namespaces = vec![
            "analytics".into(),
            "another-ns".into(),
            "default".into(),
            "kube-system".into(),
        ];

        let results = filter_namespaces(&namespaces, OsStr::new("an"));
        let names = candidate_values(&results);

        assert_eq!(names, vec!["analytics", "another-ns"]);
    }

    #[test]
    fn filter_namespaces_empty_prefix_returns_all() {
        let namespaces = vec!["default".into(), "kube-system".into(), "monitoring".into()];

        let results = filter_namespaces(&namespaces, OsStr::new(""));
        let names = candidate_values(&results);

        assert_eq!(names, vec!["default", "kube-system", "monitoring"]);
    }

    #[test]
    fn filter_namespaces_no_match_returns_empty() {
        let namespaces = vec!["default".into(), "kube-system".into()];

        let results = filter_namespaces(&namespaces, OsStr::new("zzz"));
        assert!(results.is_empty());
    }

    #[test]
    fn filter_namespaces_exact_match() {
        let namespaces = vec!["default".into(), "kube-system".into()];

        let results = filter_namespaces(&namespaces, OsStr::new("default"));
        let names = candidate_values(&results);

        assert_eq!(names, vec!["default"]);
    }

    #[test]
    fn filter_namespaces_case_sensitive() {
        let namespaces = vec!["Analytics".into(), "analytics".into()];

        let results = filter_namespaces(&namespaces, OsStr::new("an"));
        let names = candidate_values(&results);

        // K8s namespaces are lowercase, but the filter is case-sensitive.
        assert_eq!(names, vec!["analytics"]);
    }

    #[test]
    fn filter_namespaces_empty_list() {
        let namespaces: Vec<String> = vec![];

        let results = filter_namespaces(&namespaces, OsStr::new("an"));
        assert!(results.is_empty());
    }

    // -- extract_flag_value ---------------------------------------------------
    // Tests exercise the shared `extract_flag_value` directly with explicit
    // arg slices (avoids depending on `std::env::args()`).

    fn args(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn extract_flag_long_space_separated() {
        let a = args(&["kube-log", "logs", "--namespace", "kube-system", "--pod"]);
        let result = extract_flag_value(&a, &["--namespace", "-n"]);
        assert_eq!(result, Some("kube-system".to_string()));
    }

    #[test]
    fn extract_flag_long_equals() {
        let a = args(&["kube-log", "logs", "--namespace=staging", "--pod"]);
        let result = extract_flag_value(&a, &["--namespace", "-n"]);
        assert_eq!(result, Some("staging".to_string()));
    }

    #[test]
    fn extract_flag_short_space_separated() {
        let a = args(&["kube-log", "logs", "-n", "monitoring", "--pod"]);
        let result = extract_flag_value(&a, &["--namespace", "-n"]);
        assert_eq!(result, Some("monitoring".to_string()));
    }

    #[test]
    fn extract_flag_short_glued() {
        let a = args(&["kube-log", "logs", "-nkube-system", "--pod"]);
        let result = extract_flag_value(&a, &["--namespace", "-n"]);
        assert_eq!(result, Some("kube-system".to_string()));
    }

    #[test]
    fn extract_flag_absent_returns_none() {
        let a = args(&["kube-log", "logs", "--pod", "my-pod"]);
        let result = extract_flag_value(&a, &["--namespace", "-n"]);
        assert_eq!(result, None);
    }

    #[test]
    fn extract_flag_context_long() {
        let a = args(&["kube-log", "logs", "--context", "prod", "-n", "default"]);
        let result = extract_flag_value(&a, &["--context"]);
        assert_eq!(result, Some("prod".to_string()));
    }

    #[test]
    fn extract_flag_value_looks_like_flag_skipped() {
        // If the "value" starts with '-', it's probably another flag, not a value.
        let a = args(&["kube-log", "logs", "-n", "--pod", "my-pod"]);
        let result = extract_flag_value(&a, &["--namespace", "-n"]);
        assert_eq!(result, None);
    }
}
