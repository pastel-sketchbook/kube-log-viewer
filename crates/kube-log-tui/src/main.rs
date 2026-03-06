#[cfg(unix)]
use std::os::unix::io::AsRawFd;

use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use kube_log_tui::app::App;
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

    // Housekeeping: keep only the 3 most recent log files from previous runs.
    // The daily appender creates files like `kube-log-viewer.log.2026-02-23`.
    if let Ok(entries) = std::fs::read_dir(&data_dir) {
        let mut files: Vec<_> = entries.flatten().filter(|e| e.path().is_file()).collect();
        // Sort by modification time, newest first
        files.sort_by(|a, b| {
            let t_a = a.metadata().and_then(|m| m.modified()).ok();
            let t_b = b.metadata().and_then(|m| m.modified()).ok();
            t_b.cmp(&t_a)
        });
        // Delete everything beyond the 3 newest
        for old in files.into_iter().skip(3) {
            let _ = std::fs::remove_file(old.path());
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
        let _ = execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        original_hook(panic_info);
    }));

    // Setup terminal
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    // Redirect stderr to /dev/null so that subprocess output (e.g. kubelogin,
    // az CLI exec plugins) does not write directly to the terminal and corrupt
    // the TUI display. Errors are captured via kube client results and shown
    // in the log pane instead.
    #[cfg(unix)]
    {
        let devnull = std::fs::File::open("/dev/null").context("failed to open /dev/null")?;
        // SAFETY: dup2 is a standard POSIX call; fd 2 (stderr) is always valid.
        unsafe {
            libc::dup2(devnull.as_raw_fd(), libc::STDERR_FILENO);
        }
    }

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
