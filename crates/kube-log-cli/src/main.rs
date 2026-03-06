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

#[tokio::main]
async fn main() -> ExitCode {
    // Handle shell completion requests (COMPLETE=bash/zsh/fish kube-log).
    // Must run before any stdout output or argument parsing.
    CompleteEnv::with_factory(cli::Cli::command).complete();

    let args = cli::Cli::parse();

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
