use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use kube_log_viewer::app::App;
use ratatui::prelude::*;
use tracing::info;

/// Initialise the tracing subscriber that writes to a daily-rotated log file.
///
/// Logs are stored under the platform data directory (macOS:
/// `~/Library/Application Support/kube-log-viewer/logs/`).
/// On each startup, previous rotated log files are deleted so only the
/// current session's log remains.
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let data_dir = dirs::data_dir()?.join("kube-log-viewer").join("logs");

    // Best-effort directory creation -- if it fails we silently skip tracing
    std::fs::create_dir_all(&data_dir).ok()?;

    // Housekeeping: remove old rotated log files from previous runs.
    // The daily appender creates files like `kube-log-viewer.log.2026-02-23`.
    // We delete everything in the directory so today's session starts fresh.
    if let Ok(entries) = std::fs::read_dir(&data_dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }

    let file_appender = tracing_appender::rolling::daily(&data_dir, "kube-log-viewer.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .init();

    Some(guard)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Start tracing before anything else. The guard must be held until exit
    // so that buffered log lines are flushed.
    let _tracing_guard = init_tracing();

    info!("kube-log-viewer starting");

    // Install panic hook that restores terminal before printing panic info
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    // Setup terminal
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    // Run app
    let result = App::run(&mut terminal).await;

    // Restore terminal
    disable_raw_mode().context("failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;
    terminal.show_cursor().context("failed to show cursor")?;

    info!("kube-log-viewer exiting");

    result
}
