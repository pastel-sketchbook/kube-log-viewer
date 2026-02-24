pub mod header;
pub mod logs;
pub mod pods;
pub mod popup;
pub mod statusbar;
pub mod theme;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;

/// Top-level render function -- draws the full UI for one frame.
pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // main body
            Constraint::Length(1), // status bar
        ])
        .split(frame.area());

    header::render(frame, app, chunks[0]);

    // Main content: pod list (left) + log viewer (right)
    let (pod_pct, log_pct) = if app.wide_logs { (10, 90) } else { (25, 75) };
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(pod_pct), // pod list
            Constraint::Percentage(log_pct), // log viewer
        ])
        .split(chunks[1]);

    pods::render(frame, app, main_chunks[0]);
    logs::render(frame, app, main_chunks[1]);
    statusbar::render(frame, app, chunks[2]);

    // Overlays (rendered on top)
    if app.popup.is_some() {
        popup::render(frame, app);
    }
    if app.show_help {
        render_help(frame, app);
    }
}

// ---------------------------------------------------------------------------
// Help overlay
// ---------------------------------------------------------------------------

fn render_help(frame: &mut Frame, app: &App) {
    let area = centered_rect(60, 70, frame.area());
    frame.render_widget(Clear, area);

    let theme = app.theme();

    let help_text = vec![
        "Keybindings:",
        "",
        "  Navigation:",
        "    j/Down     Move down",
        "    k/Up       Move up",
        "    Enter      Select pod / start log stream",
        "    Tab        Switch focus (Pods <-> Logs)",
        "    g          Scroll to top (logs)",
        "    G          Scroll to bottom (logs)",
        "    PgUp/PgDn  Page up/down (logs)",
        "",
        "  Actions:",
        "    n          Switch namespace",
        "    c          Switch context",
        "    s          Switch container",
        "    /          Search/filter logs",
        "    f          Toggle follow mode",
        "    w          Toggle wide log view",
        "    W          Toggle line wrap",
        "    J          Toggle JSON formatting",
        "    t          Cycle theme",
        "",
        "  General:",
        "    ?          Toggle this help",
        "    q          Quit",
        "    Ctrl+C     Force quit",
        "    Esc        Close popup / clear search",
    ];

    let paragraph = Paragraph::new(help_text.join("\n"))
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.popup_border)),
        )
        .style(Style::default().fg(theme.fg));

    frame.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Layout helpers
// ---------------------------------------------------------------------------

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
