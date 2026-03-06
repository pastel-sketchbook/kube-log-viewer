//! `kube-log` — LLM-facing CLI for Kubernetes log analysis.
//!
//! Fetches, classifies, and reduces pod logs through an anomaly detection
//! pipeline (classify → filter → reduce → export). Outputs pre-triaged JSON
//! for piping into LLM tools.
//!
//! ```bash
//! kube-log logs --pod payments-7f8d --time-range 15m \
//!   | opencode run "Diagnose this Kubernetes incident"
//! ```

use std::process::ExitCode;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::CompleteEnv;

mod cli;
mod complete;
mod output;

/// Known subcommand names and aliases.
const SUBCOMMANDS: &[&str] = &["logs", "pods", "po", "contexts", "ctx", "namespaces", "ns"];

/// Pre-process argv to handle bare `-n` (without a value) as a shortcut for
/// the `namespaces` subcommand. Only applies when no explicit subcommand is
/// present — `kube-log logs -n` is left alone for clap to handle normally.
fn preprocess_args(mut args: Vec<String>) -> Vec<String> {
    let has_subcommand = args
        .iter()
        .skip(1)
        .any(|a| SUBCOMMANDS.contains(&a.as_str()));
    if has_subcommand {
        return args;
    }

    if let Some(pos) = args.iter().position(|a| a == "-n") {
        let next_is_value = args
            .get(pos + 1)
            .is_some_and(|s| !s.starts_with('-'));
        if !next_is_value {
            // Bare `-n` → rewrite to `ns` subcommand.
            args.remove(pos);
            args.insert(1, "ns".to_string());
        }
    }
    args
}

#[tokio::main]
async fn main() -> ExitCode {
    // Handle shell completion requests (COMPLETE=bash/zsh/fish kube-log).
    // Must run before any stdout output or argument parsing.
    CompleteEnv::with_factory(cli::Cli::command).complete();

    let args = cli::Cli::parse_from(preprocess_args(std::env::args().collect()));

    if let Err(e) = run(args).await {
        // Structured error output for LLM consumption.
        let error = serde_json::json!({
            "error": "cli_error",
            "message": format!("{e:#}"),
        });
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&error).unwrap_or_else(|_| format!("{e:#}"))
        );
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

async fn run(args: cli::Cli) -> Result<()> {
    match args.command.unwrap_or(cli::Command::Logs(args.logs_args)) {
        cli::Command::Logs(args) => output::run_logs(args).await,
        cli::Command::Pods(args) => output::run_pods(args).await,
        cli::Command::Contexts => output::run_contexts().await,
        cli::Command::Namespaces(args) => output::run_namespaces(args).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn bare_n_rewrites_to_ns() {
        let result = preprocess_args(args(&["kube-log", "-n"]));
        assert_eq!(result, args(&["kube-log", "ns"]));
    }

    #[test]
    fn bare_n_with_context_rewrites_to_ns() {
        let result = preprocess_args(args(&["kube-log", "-n", "--context", "prod"]));
        assert_eq!(result, args(&["kube-log", "ns", "--context", "prod"]));
    }

    #[test]
    fn n_with_value_is_unchanged() {
        let input = args(&["kube-log", "-n", "kube-system", "--pod", "foo"]);
        let result = preprocess_args(input.clone());
        assert_eq!(result, input);
    }

    #[test]
    fn n_within_explicit_subcommand_is_unchanged() {
        let input = args(&["kube-log", "logs", "-n", "kube-system"]);
        let result = preprocess_args(input.clone());
        assert_eq!(result, input);
    }

    #[test]
    fn no_n_flag_is_unchanged() {
        let input = args(&["kube-log", "--pod", "foo"]);
        let result = preprocess_args(input.clone());
        assert_eq!(result, input);
    }
}
