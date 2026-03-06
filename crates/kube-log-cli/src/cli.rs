//! CLI argument definitions for `kube-log`.
//!
//! Uses `clap` derive macros. The default subcommand is `logs` — when no
//! subcommand is given, the logs arguments are parsed directly from the
//! top-level flags.

use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::engine::ArgValueCompleter;

use kube_log_core::types::{FilterConfig, OutputFormat};

use crate::complete;

// ---------------------------------------------------------------------------
// Top-level CLI
// ---------------------------------------------------------------------------

/// LLM-facing CLI for Kubernetes log analysis with anomaly detection.
///
/// Fetches, classifies, and reduces pod logs. Outputs pre-triaged JSON for
/// piping into LLM tools like `opencode run` or `copilot -p`.
#[derive(Debug, Parser)]
#[command(name = "kube-log", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Flatten logs args at top level so `kube-log --pod foo` works without
    /// the `logs` subcommand.
    #[command(flatten)]
    pub logs_args: LogsArgs,
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Fetch and analyze pod logs (default).
    Logs(LogsArgs),

    /// List pods with status.
    #[command(alias = "po")]
    Pods(PodsArgs),

    /// List available K8s contexts.
    #[command(alias = "ctx")]
    Contexts,

    /// List namespaces.
    #[command(alias = "ns")]
    Namespaces(NamespacesArgs),
}

// ---------------------------------------------------------------------------
// Logs args
// ---------------------------------------------------------------------------

/// Arguments for the `logs` subcommand (and top-level default).
#[derive(Debug, Clone, Parser)]
pub struct LogsArgs {
    /// Kubernetes context (default: current context from kubeconfig).
    #[arg(long, add = ArgValueCompleter::new(complete::context_completer))]
    pub context: Option<String>,

    /// Namespace (default: current namespace from kubeconfig).
    #[arg(long, short = 'n', add = ArgValueCompleter::new(complete::namespace_completer))]
    pub namespace: Option<String>,

    /// Pod name(s). Omit to analyze all pods in the namespace.
    #[arg(long, short = 'p', required = false, add = ArgValueCompleter::new(complete::pod_completer))]
    pub pod: Vec<String>,

    /// Container name (optional, for multi-container pods).
    #[arg(long, short = 'c')]
    pub container: Option<String>,

    /// Number of recent lines to fetch per pod (default: 1000).
    #[arg(long, default_value_t = 1000)]
    pub lines: i64,

    /// Stream logs continuously (JSON-lines to stdout).
    #[arg(long, short = 'f')]
    pub follow: bool,

    /// Only include lines from last N duration (e.g. 15m, 1h, 6h).
    #[arg(long)]
    pub time_range: Option<String>,

    /// Filter lines by substring match.
    #[arg(long, short = 's')]
    pub search: Option<String>,

    /// Output summary + filtered lines (default for non-TTY).
    #[arg(long)]
    pub summary: bool,

    /// Disable troubleshoot filtering, show all lines.
    #[arg(long)]
    pub all: bool,

    /// Include Normal-class lines (but still suppress health checks).
    #[arg(long)]
    pub verbose: bool,

    /// Comma-separated classes to include: error,warning,lifecycle,novel,normal,healthcheck.
    #[arg(long)]
    pub include: Option<String>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = FormatArg::Json)]
    pub format: FormatArg,
}

impl LogsArgs {
    /// Derive a [`FilterConfig`] from the CLI flags.
    pub fn filter_config(&self) -> FilterConfig {
        if self.all {
            FilterConfig::all()
        } else if let Some(ref include) = self.include {
            FilterConfig::from_include_list(include)
        } else if self.verbose {
            FilterConfig::verbose()
        } else {
            FilterConfig::troubleshoot()
        }
    }

    /// Derive an [`OutputFormat`] from the CLI flags.
    pub fn output_format(&self) -> OutputFormat {
        match self.format {
            FormatArg::Json => OutputFormat::Json,
            FormatArg::Jsonl => OutputFormat::Jsonl,
            FormatArg::Plain => OutputFormat::Plain,
        }
    }

    /// Parse `--time-range` into a [`jiff::SignedDuration`].
    ///
    /// Supports suffixes: `s` (seconds), `m` (minutes), `h` (hours), `d` (days).
    /// Returns `None` if no `--time-range` was specified.
    pub fn parse_time_range(&self) -> anyhow::Result<Option<jiff::SignedDuration>> {
        let Some(ref raw) = self.time_range else {
            return Ok(None);
        };

        let raw = raw.trim();
        if raw.is_empty() {
            return Ok(None);
        }

        let (digits, suffix) =
            raw.split_at(raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len()));

        let n: i64 = digits
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid --time-range value: '{raw}'"))?;

        let duration = match suffix {
            "s" | "" => jiff::SignedDuration::from_secs(n),
            "m" => jiff::SignedDuration::from_secs(n * 60),
            "h" => jiff::SignedDuration::from_secs(n * 3600),
            "d" => jiff::SignedDuration::from_secs(n * 86400),
            _ => anyhow::bail!("unknown --time-range suffix '{suffix}' in '{raw}' (use s/m/h/d)"),
        };

        Ok(Some(duration))
    }
}

// ---------------------------------------------------------------------------
// Pods args
// ---------------------------------------------------------------------------

/// Arguments for the `pods` subcommand.
#[derive(Debug, Clone, Parser)]
pub struct PodsArgs {
    /// Kubernetes context (default: current).
    #[arg(long, add = ArgValueCompleter::new(complete::context_completer))]
    pub context: Option<String>,

    /// Namespace (default: current).
    #[arg(long, short = 'n', add = ArgValueCompleter::new(complete::namespace_completer))]
    pub namespace: Option<String>,
}

// ---------------------------------------------------------------------------
// Namespaces args
// ---------------------------------------------------------------------------

/// Arguments for the `namespaces` subcommand.
#[derive(Debug, Clone, Parser)]
pub struct NamespacesArgs {
    /// Kubernetes context (default: current).
    #[arg(long, add = ArgValueCompleter::new(complete::context_completer))]
    pub context: Option<String>,
}

// ---------------------------------------------------------------------------
// Format enum (clap-compatible)
// ---------------------------------------------------------------------------

/// Output format for the CLI.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum FormatArg {
    /// Single JSON document with summary + lines (default).
    Json,
    /// One JSON object per line (streaming-friendly).
    Jsonl,
    /// Plain text (human-readable fallback).
    Plain,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_config_default_is_troubleshoot() {
        let args = LogsArgs {
            context: None,
            namespace: None,
            pod: vec![],
            container: None,
            lines: 1000,
            follow: false,
            time_range: None,
            search: None,
            summary: false,
            all: false,
            verbose: false,
            include: None,
            format: FormatArg::Json,
        };
        let cfg = args.filter_config();
        assert!(cfg.should_include(&kube_log_core::types::LineClass::Error));
        assert!(!cfg.should_include(&kube_log_core::types::LineClass::Normal));
        assert!(!cfg.should_include(&kube_log_core::types::LineClass::HealthCheck));
    }

    #[test]
    fn test_filter_config_all() {
        let args = LogsArgs {
            context: None,
            namespace: None,
            pod: vec![],
            container: None,
            lines: 1000,
            follow: false,
            time_range: None,
            search: None,
            summary: false,
            all: true,
            verbose: false,
            include: None,
            format: FormatArg::Json,
        };
        let cfg = args.filter_config();
        assert!(cfg.should_include(&kube_log_core::types::LineClass::HealthCheck));
        assert!(cfg.should_include(&kube_log_core::types::LineClass::Normal));
    }

    #[test]
    fn test_filter_config_verbose() {
        let args = LogsArgs {
            context: None,
            namespace: None,
            pod: vec![],
            container: None,
            lines: 1000,
            follow: false,
            time_range: None,
            search: None,
            summary: false,
            all: false,
            verbose: true,
            include: None,
            format: FormatArg::Json,
        };
        let cfg = args.filter_config();
        assert!(cfg.should_include(&kube_log_core::types::LineClass::Normal));
        assert!(!cfg.should_include(&kube_log_core::types::LineClass::HealthCheck));
    }

    #[test]
    fn test_filter_config_custom_include() {
        let args = LogsArgs {
            context: None,
            namespace: None,
            pod: vec![],
            container: None,
            lines: 1000,
            follow: false,
            time_range: None,
            search: None,
            summary: false,
            all: false,
            verbose: false,
            include: Some("error,lifecycle".into()),
            format: FormatArg::Json,
        };
        let cfg = args.filter_config();
        assert!(cfg.should_include(&kube_log_core::types::LineClass::Error));
        assert!(cfg.should_include(&kube_log_core::types::LineClass::Lifecycle));
        assert!(!cfg.should_include(&kube_log_core::types::LineClass::Warning));
    }

    #[test]
    fn test_parse_time_range_minutes() {
        let args = LogsArgs {
            context: None,
            namespace: None,
            pod: vec![],
            container: None,
            lines: 1000,
            follow: false,
            time_range: Some("15m".into()),
            search: None,
            summary: false,
            all: false,
            verbose: false,
            include: None,
            format: FormatArg::Json,
        };
        let dur = args.parse_time_range().unwrap().unwrap();
        assert_eq!(dur.as_secs() / 60, 15);
    }

    #[test]
    fn test_parse_time_range_hours() {
        let args = LogsArgs {
            context: None,
            namespace: None,
            pod: vec![],
            container: None,
            lines: 1000,
            follow: false,
            time_range: Some("6h".into()),
            search: None,
            summary: false,
            all: false,
            verbose: false,
            include: None,
            format: FormatArg::Json,
        };
        let dur = args.parse_time_range().unwrap().unwrap();
        assert_eq!(dur.as_secs() / 3600, 6);
    }

    #[test]
    fn test_parse_time_range_none() {
        let args = LogsArgs {
            context: None,
            namespace: None,
            pod: vec![],
            container: None,
            lines: 1000,
            follow: false,
            time_range: None,
            search: None,
            summary: false,
            all: false,
            verbose: false,
            include: None,
            format: FormatArg::Json,
        };
        assert!(args.parse_time_range().unwrap().is_none());
    }

    #[test]
    fn test_parse_time_range_invalid_suffix() {
        let args = LogsArgs {
            context: None,
            namespace: None,
            pod: vec![],
            container: None,
            lines: 1000,
            follow: false,
            time_range: Some("15x".into()),
            search: None,
            summary: false,
            all: false,
            verbose: false,
            include: None,
            format: FormatArg::Json,
        };
        assert!(args.parse_time_range().is_err());
    }

    #[test]
    fn test_output_format_default_is_json() {
        let args = LogsArgs {
            context: None,
            namespace: None,
            pod: vec![],
            container: None,
            lines: 1000,
            follow: false,
            time_range: None,
            search: None,
            summary: false,
            all: false,
            verbose: false,
            include: None,
            format: FormatArg::Json,
        };
        assert_eq!(args.output_format(), OutputFormat::Json);
    }

    #[test]
    fn test_output_format_jsonl() {
        let args = LogsArgs {
            context: None,
            namespace: None,
            pod: vec![],
            container: None,
            lines: 1000,
            follow: false,
            time_range: None,
            search: None,
            summary: false,
            all: false,
            verbose: false,
            include: None,
            format: FormatArg::Jsonl,
        };
        assert_eq!(args.output_format(), OutputFormat::Jsonl);
    }
}
