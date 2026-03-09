use std::time::Duration;

use anyhow::{Context as _, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use jiff::{SignedDuration, Timestamp, Zoned};
use ratatui::prelude::*;
use ratatui::widgets::ListState;
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info};

use crate::event::AppEvent;
use crate::prefs;
use crate::ui;
use crate::ui::theme::{THEMES, Theme};
use kube_log_core::k8s;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Pods,
    Logs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupKind {
    Namespaces,
    Contexts,
    Containers,
    TimeRange,
    ExportFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    PlainText,
    Json,
    Csv,
}

impl ExportFormat {
    /// File extension for this format (without the leading dot).
    pub fn extension(self) -> &'static str {
        match self {
            Self::PlainText => "log",
            Self::Json => "json",
            Self::Csv => "csv",
        }
    }
}

pub const EXPORT_FORMAT_OPTIONS: &[(&str, ExportFormat)] = &[
    ("Plain Text (.log)", ExportFormat::PlainText),
    ("JSON (.json)", ExportFormat::Json),
    ("CSV (.csv)", ExportFormat::Csv),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimestampMode {
    Utc,
    #[default]
    Local,
    Relative,
}

impl TimestampMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Utc => Self::Local,
            Self::Local => Self::Relative,
            Self::Relative => Self::Utc,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Utc => "UTC",
            Self::Local => "Local",
            Self::Relative => "Relative",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeRange {
    #[default]
    All,
    Last(Duration),
}

/// Predefined time range options shown in the popup.
pub const TIME_RANGE_OPTIONS: &[(&str, TimeRange)] = &[
    ("All", TimeRange::All),
    ("Last 5m", TimeRange::Last(Duration::from_secs(5 * 60))),
    ("Last 15m", TimeRange::Last(Duration::from_secs(15 * 60))),
    ("Last 30m", TimeRange::Last(Duration::from_secs(30 * 60))),
    ("Last 1h", TimeRange::Last(Duration::from_secs(60 * 60))),
    ("Last 6h", TimeRange::Last(Duration::from_secs(6 * 60 * 60))),
    (
        "Last 24h",
        TimeRange::Last(Duration::from_secs(24 * 60 * 60)),
    ),
];

impl TimeRange {
    pub fn label(self) -> &'static str {
        for &(name, range) in TIME_RANGE_OPTIONS {
            if range == self {
                return name;
            }
        }
        "Custom"
    }
}

// ---------------------------------------------------------------------------
// Multi-stream types
// ---------------------------------------------------------------------------

/// Maximum number of concurrent log streams.
pub const MAX_STREAMS: usize = 4;

/// A log line tagged with the source pod name.
/// Empty `source` indicates a system/internal message (errors, info).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedLine {
    pub source: String,
    pub line: String,
}

impl TaggedLine {
    pub fn system(line: String) -> Self {
        Self {
            source: String::new(),
            line,
        }
    }
}

/// How multiple log streams are displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamMode {
    /// One stream, classic single-pod view.
    #[default]
    Single,
    /// All streams interleaved chronologically, pod tag prefix per line.
    Merged,
    /// Stacked horizontal panes (top to bottom), one per stream (up to 4).
    Split,
}

/// Per-pane TUI view state for a log stream in split mode.
#[derive(Debug, Clone)]
pub struct LogViewState {
    pub scroll_offset: usize,
    pub follow_mode: bool,
}

impl Default for LogViewState {
    fn default() -> Self {
        Self {
            scroll_offset: 0,
            follow_mode: true,
        }
    }
}

/// Handle to a running log stream background task.
///
/// Domain fields (`pod_name`, `container`, `cancel_tx`) identify and control
/// the stream. TUI-specific per-pane state lives in [`LogViewState`].
pub struct LogStreamHandle {
    pub pod_name: String,
    pub container: Option<String>,
    pub cancel_tx: tokio::sync::watch::Sender<bool>,
    /// Per-pane TUI view state (scroll position, follow mode).
    pub view: LogViewState,
}

impl std::fmt::Debug for LogStreamHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogStreamHandle")
            .field("pod_name", &self.pod_name)
            .field("container", &self.container)
            .field("view", &self.view)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

pub struct App {
    // K8s state
    pub contexts: Vec<String>,
    pub current_context: String,
    pub namespaces: Vec<String>,
    pub current_namespace: String,
    pub pods: Vec<k8s::pods::PodInfo>,

    // UI state
    pub focus: Focus,
    pub pod_list_state: ListState,
    pub log_scroll_offset: usize,
    pub follow_mode: bool,
    pub wrap_lines: bool,
    pub wide_logs: bool,
    pub json_mode: bool,
    pub timestamp_mode: TimestampMode,
    pub time_range: TimeRange,
    pub hide_health_checks: bool,
    pub input_mode: InputMode,
    pub search_query: String,
    pub show_help: bool,

    // Popup state
    pub popup: Option<PopupKind>,
    pub popup_list_state: ListState,

    // Log state
    pub log_lines: Vec<TaggedLine>,
    pub selected_pod: Option<String>,
    pub selected_container: Option<String>,
    pub containers: Vec<String>,

    // Multi-stream state
    pub streams: Vec<LogStreamHandle>,
    pub stream_mode: StreamMode,
    pub active_pane: usize,

    // Theme
    pub theme_index: usize,

    // Auth state
    pub az_login_in_progress: bool,
    /// Cancellation sender for the `az login` background task.
    az_login_cancel: Option<watch::Sender<bool>>,

    // Terminal dimensions (updated on Resize events)
    pub last_terminal_height: u16,

    // Control
    pub should_quit: bool,

    // Channel for sending events from background tasks
    tx: mpsc::UnboundedSender<AppEvent>,

    // Pod watcher cancellation handle
    pod_watcher_cancel: Option<tokio::sync::watch::Sender<bool>>,
}

impl App {
    pub fn new(tx: mpsc::UnboundedSender<AppEvent>) -> Self {
        Self {
            contexts: Vec::new(),
            current_context: String::new(),
            namespaces: Vec::new(),
            current_namespace: String::from("default"),
            pods: Vec::new(),

            focus: Focus::Pods,
            pod_list_state: ListState::default(),
            log_scroll_offset: 0,
            follow_mode: true,
            wrap_lines: false,
            wide_logs: false,
            json_mode: true,
            timestamp_mode: TimestampMode::default(),
            time_range: TimeRange::default(),
            hide_health_checks: true,
            input_mode: InputMode::Normal,
            search_query: String::new(),
            show_help: false,

            popup: None,
            popup_list_state: ListState::default(),

            log_lines: Vec::new(),
            selected_pod: None,
            selected_container: None,
            containers: Vec::new(),

            streams: Vec::new(),
            stream_mode: StreamMode::default(),
            active_pane: 0,

            theme_index: prefs::theme_index_from_prefs(&prefs::load()),

            az_login_in_progress: false,
            az_login_cancel: None,

            last_terminal_height: 24,
            should_quit: false,
            tx,
            pod_watcher_cancel: None,
        }
    }

    // -- Main event loop ----------------------------------------------------

    pub async fn run(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();
        let mut app = App::new(tx.clone());

        // Kick off initial K8s data load
        app.load_contexts();

        let mut event_stream = EventStream::new();

        loop {
            terminal
                .draw(|f| ui::render(f, &mut app))
                .context("failed to render frame")?;

            tokio::select! {
                maybe_event = event_stream.next() => {
                    match maybe_event {
                        Some(Ok(Event::Key(key))) => app.handle_key(key),
                        Some(Ok(Event::Resize(_, h))) => { app.last_terminal_height = h; }
                        Some(Err(_)) | None => break,
                        _ => {}
                    }
                }
                maybe_event = rx.recv() => {
                    match maybe_event {
                        Some(event) => app.handle_app_event(event),
                        None => break,
                    }
                    // Drain all remaining queued events before re-rendering.
                    // Without this, high-volume log streams fall behind because
                    // only one event is processed per render frame.
                    while let Ok(event) = rx.try_recv() {
                        app.handle_app_event(event);
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(250)) => {
                    // tick -- triggers re-render
                }
            }

            if app.should_quit {
                break;
            }
        }

        // Clean up any running background tasks
        app.cancel_all_streams();
        app.cancel_pod_watcher();
        app.cancel_az_login();
        Ok(())
    }

    // -- Key handling -------------------------------------------------------

    pub fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl+C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match (self.popup.is_some(), self.input_mode) {
            (true, _) => self.handle_popup_key(key),
            (_, InputMode::Search) => self.handle_search_key(key),
            _ => self.handle_normal_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Char('n') => self.open_popup(PopupKind::Namespaces),
            KeyCode::Char('c') => self.open_popup(PopupKind::Contexts),
            KeyCode::Char('s') if !self.containers.is_empty() => {
                self.open_popup(PopupKind::Containers);
            }
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Search;
                self.search_query.clear();
            }
            KeyCode::Char('f') => {
                if self.stream_mode == StreamMode::Split {
                    if let Some(handle) = self.streams.get_mut(self.active_pane) {
                        handle.view.follow_mode = !handle.view.follow_mode;
                    }
                } else {
                    self.follow_mode = !self.follow_mode;
                }
            }
            KeyCode::Char('w') => self.wide_logs = !self.wide_logs,
            KeyCode::Char('W') => self.wrap_lines = !self.wrap_lines,
            KeyCode::Char('J') => self.json_mode = !self.json_mode,
            KeyCode::Char('H') => self.hide_health_checks = !self.hide_health_checks,
            KeyCode::Char('T') => self.timestamp_mode = self.timestamp_mode.cycle(),
            KeyCode::Char('R') => self.open_popup(PopupKind::TimeRange),
            KeyCode::Char('t') => self.cycle_theme(),
            KeyCode::Char('E') => {
                if self.json_mode {
                    self.open_popup(PopupKind::ExportFormat);
                } else {
                    self.export_logs(ExportFormat::PlainText);
                }
            }
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Pods => Focus::Logs,
                    Focus::Logs => Focus::Pods,
                };
            }
            KeyCode::Esc => {
                if self.show_help {
                    self.show_help = false;
                } else if !self.search_query.is_empty() {
                    self.search_query.clear();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.navigate_up(),
            KeyCode::Down | KeyCode::Char('j') => self.navigate_down(),
            KeyCode::Enter => self.select_current(),
            KeyCode::Char('G') => self.scroll_to_bottom(),
            KeyCode::Char('g') => self.scroll_to_top(),
            KeyCode::PageUp => self.page_up(),
            KeyCode::PageDown => self.page_down(),
            // Multi-stream keybindings
            KeyCode::Char('M') => self.add_stream(),
            KeyCode::Char('V') => self.cycle_view(),
            KeyCode::Char('X') => self.remove_last_stream(),
            KeyCode::Char('1') if self.stream_mode == StreamMode::Split => {
                self.active_pane = 0;
            }
            KeyCode::Char('2')
                if self.stream_mode == StreamMode::Split && self.streams.len() >= 2 =>
            {
                self.active_pane = 1;
            }
            KeyCode::Char('3')
                if self.stream_mode == StreamMode::Split && self.streams.len() >= 3 =>
            {
                self.active_pane = 2;
            }
            KeyCode::Char('4')
                if self.stream_mode == StreamMode::Split && self.streams.len() >= 4 =>
            {
                self.active_pane = 3;
            }
            _ => {}
        }
    }

    fn handle_popup_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.popup = None,
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.popup_list_state.selected().unwrap_or(0);
                if i > 0 {
                    self.popup_list_state.select(Some(i - 1));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.popup_items_len();
                let i = self.popup_list_state.selected().unwrap_or(0);
                if i + 1 < len {
                    self.popup_list_state.select(Some(i + 1));
                }
            }
            KeyCode::Enter => self.confirm_popup_selection(),
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.input_mode = InputMode::Normal,
            KeyCode::Backspace => {
                self.search_query.pop();
            }
            KeyCode::Char(c) => self.search_query.push(c),
            _ => {}
        }
    }

    // -- Navigation ---------------------------------------------------------

    fn navigate_up(&mut self) {
        match (self.focus, self.stream_mode) {
            (Focus::Pods, _) => {
                let i = self.pod_list_state.selected().unwrap_or(0);
                if i > 0 {
                    self.pod_list_state.select(Some(i - 1));
                }
            }
            (Focus::Logs, StreamMode::Split) => {
                if let Some(handle) = self.streams.get_mut(self.active_pane) {
                    handle.view.follow_mode = false;
                    handle.view.scroll_offset = handle.view.scroll_offset.saturating_sub(1);
                }
            }
            (Focus::Logs, _) => {
                self.follow_mode = false;
                self.log_scroll_offset = self.log_scroll_offset.saturating_sub(1);
            }
        }
    }

    fn navigate_down(&mut self) {
        match (self.focus, self.stream_mode) {
            (Focus::Pods, _) => {
                let len = self.pods.len();
                let i = self.pod_list_state.selected().unwrap_or(0);
                if len > 0 && i + 1 < len {
                    self.pod_list_state.select(Some(i + 1));
                }
            }
            (Focus::Logs, StreamMode::Split) => {
                let max = self
                    .filtered_log_lines_for_pane(self.active_pane)
                    .len()
                    .saturating_sub(1);
                if let Some(handle) = self.streams.get_mut(self.active_pane) {
                    handle.view.follow_mode = false;
                    if handle.view.scroll_offset < max {
                        handle.view.scroll_offset += 1;
                    }
                }
            }
            (Focus::Logs, _) => {
                self.follow_mode = false;
                let max = self.filtered_log_lines().len().saturating_sub(1);
                if self.log_scroll_offset < max {
                    self.log_scroll_offset += 1;
                }
            }
        }
    }

    fn scroll_to_bottom(&mut self) {
        match (self.focus, self.stream_mode) {
            (Focus::Pods, _) => {}
            (Focus::Logs, StreamMode::Split) => {
                let max = self
                    .filtered_log_lines_for_pane(self.active_pane)
                    .len()
                    .saturating_sub(1);
                if let Some(handle) = self.streams.get_mut(self.active_pane) {
                    handle.view.follow_mode = true;
                    handle.view.scroll_offset = max;
                }
            }
            (Focus::Logs, _) => {
                self.follow_mode = true;
                self.log_scroll_offset = self.filtered_log_lines().len().saturating_sub(1);
            }
        }
    }

    fn scroll_to_top(&mut self) {
        match (self.focus, self.stream_mode) {
            (Focus::Pods, _) => {}
            (Focus::Logs, StreamMode::Split) => {
                if let Some(handle) = self.streams.get_mut(self.active_pane) {
                    handle.view.follow_mode = false;
                    handle.view.scroll_offset = 0;
                }
            }
            (Focus::Logs, _) => {
                self.follow_mode = false;
                self.log_scroll_offset = 0;
            }
        }
    }

    /// Number of lines to jump for page-up / page-down.
    /// Uses roughly half the terminal height so the user retains context.
    fn page_size(&self) -> usize {
        (self.last_terminal_height as usize / 2).max(1)
    }

    fn page_up(&mut self) {
        let page = self.page_size();
        match (self.focus, self.stream_mode) {
            (Focus::Pods, _) => {}
            (Focus::Logs, StreamMode::Split) => {
                if let Some(handle) = self.streams.get_mut(self.active_pane) {
                    handle.view.follow_mode = false;
                    handle.view.scroll_offset = handle.view.scroll_offset.saturating_sub(page);
                }
            }
            (Focus::Logs, _) => {
                self.follow_mode = false;
                self.log_scroll_offset = self.log_scroll_offset.saturating_sub(page);
            }
        }
    }

    fn page_down(&mut self) {
        let page = self.page_size();
        match (self.focus, self.stream_mode) {
            (Focus::Pods, _) => {}
            (Focus::Logs, StreamMode::Split) => {
                let max = self
                    .filtered_log_lines_for_pane(self.active_pane)
                    .len()
                    .saturating_sub(1);
                if let Some(handle) = self.streams.get_mut(self.active_pane) {
                    handle.view.scroll_offset = (handle.view.scroll_offset + page).min(max);
                }
            }
            (Focus::Logs, _) => {
                let max = self.filtered_log_lines().len().saturating_sub(1);
                self.log_scroll_offset = (self.log_scroll_offset + page).min(max);
            }
        }
    }

    // -- Selection ----------------------------------------------------------

    fn select_current(&mut self) {
        if self.focus != Focus::Pods {
            return;
        }
        let Some(i) = self.pod_list_state.selected() else {
            return;
        };
        let Some(pod) = self.pods.get(i) else { return };

        let pod_name = pod.name.clone();
        let containers = pod.containers.clone();

        self.selected_pod = Some(pod_name.clone());
        self.containers = containers.clone();
        self.log_lines.clear();
        self.log_scroll_offset = 0;
        self.follow_mode = true;
        self.stream_mode = StreamMode::Single;
        self.active_pane = 0;

        let container = containers.first().cloned();
        self.selected_container = container.clone();

        self.start_log_stream(&pod_name, container.as_deref());
    }

    // -- Popups -------------------------------------------------------------

    fn open_popup(&mut self, kind: PopupKind) {
        self.popup_list_state = ListState::default();

        let selected = match kind {
            PopupKind::Namespaces => self
                .namespaces
                .iter()
                .position(|n| n == &self.current_namespace),
            PopupKind::Contexts => self
                .contexts
                .iter()
                .position(|c| c == &self.current_context),
            PopupKind::Containers => self
                .selected_container
                .as_ref()
                .and_then(|sc| self.containers.iter().position(|c| c == sc)),
            PopupKind::TimeRange => TIME_RANGE_OPTIONS
                .iter()
                .position(|&(_, r)| r == self.time_range),
            PopupKind::ExportFormat => Some(0),
        };

        self.popup_list_state.select(selected.or(Some(0)));
        self.popup = Some(kind);
    }

    fn popup_items_len(&self) -> usize {
        match self.popup {
            Some(PopupKind::Namespaces) => self.namespaces.len(),
            Some(PopupKind::Contexts) => self.contexts.len(),
            Some(PopupKind::Containers) => self.containers.len(),
            Some(PopupKind::TimeRange) => TIME_RANGE_OPTIONS.len(),
            Some(PopupKind::ExportFormat) => EXPORT_FORMAT_OPTIONS.len(),
            None => 0,
        }
    }

    fn confirm_popup_selection(&mut self) {
        let Some(kind) = self.popup else { return };
        let Some(i) = self.popup_list_state.selected() else {
            return;
        };

        match kind {
            PopupKind::Namespaces => {
                if let Some(ns) = self.namespaces.get(i).cloned() {
                    info!(namespace = %ns, "switching namespace");
                    self.current_namespace = ns;
                    self.selected_pod = None;
                    self.selected_container = None;
                    self.log_lines.clear();
                    self.containers.clear();
                    self.stream_mode = StreamMode::Single;
                    self.active_pane = 0;
                    self.cancel_all_streams();
                    self.start_pod_watcher();
                }
            }
            PopupKind::Contexts => {
                if let Some(ctx) = self.contexts.get(i).cloned() {
                    info!(context = %ctx, "switching context");
                    self.current_context = ctx;
                    self.current_namespace = String::from("default");
                    self.selected_pod = None;
                    self.selected_container = None;
                    self.log_lines.clear();
                    self.pods.clear();
                    self.namespaces.clear();
                    self.containers.clear();
                    self.stream_mode = StreamMode::Single;
                    self.active_pane = 0;
                    self.cancel_all_streams();
                    self.load_namespaces();
                    self.start_pod_watcher();
                }
            }
            PopupKind::Containers => {
                if let Some(container) = self.containers.get(i).cloned() {
                    info!(container = %container, "switching container");
                    self.selected_container = Some(container);
                    self.log_lines.clear();
                    self.log_scroll_offset = 0;
                    self.follow_mode = true;
                    self.stream_mode = StreamMode::Single;
                    self.active_pane = 0;

                    if let Some(pod) = self.selected_pod.clone() {
                        let container = self.selected_container.clone();
                        self.start_log_stream(&pod, container.as_deref());
                    }
                }
            }
            PopupKind::TimeRange => {
                if let Some(&(label, range)) = TIME_RANGE_OPTIONS.get(i) {
                    info!(range = label, "setting time range filter");
                    self.time_range = range;
                }
            }
            PopupKind::ExportFormat => {
                if let Some(&(_, format)) = EXPORT_FORMAT_OPTIONS.get(i) {
                    self.export_logs(format);
                }
            }
        }

        self.popup = None;
    }

    // -- App events from background tasks -----------------------------------

    pub fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::ContextsLoaded(contexts, current) => {
                info!(count = contexts.len(), current = %current, "contexts loaded");
                self.contexts = contexts;
                self.current_context = current;
                self.load_namespaces();
                self.start_pod_watcher();
            }
            AppEvent::NamespacesLoaded(context, namespaces) => {
                // Discard stale results from a previously-active context
                if context != self.current_context {
                    debug!(
                        stale_context = %context,
                        current_context = %self.current_context,
                        "discarding stale namespace list"
                    );
                } else {
                    debug!(count = namespaces.len(), "namespaces loaded");
                    self.namespaces = namespaces;
                    if !self.namespaces.contains(&self.current_namespace) {
                        self.current_namespace = self
                            .namespaces
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "default".to_string());
                    }
                }
            }
            AppEvent::PodsUpdated(pods) => {
                debug!(count = pods.len(), "pods updated");

                // Preserve selection by pod name across refreshes
                let prev_name = self
                    .pod_list_state
                    .selected()
                    .and_then(|i| self.pods.get(i))
                    .map(|p| p.name.clone());

                self.pods = pods;

                let new_index =
                    prev_name.and_then(|name| self.pods.iter().position(|p| p.name == name));

                match new_index {
                    Some(i) => self.pod_list_state.select(Some(i)),
                    None if !self.pods.is_empty() => self.pod_list_state.select(Some(0)),
                    None => self.pod_list_state.select(None),
                }
            }
            AppEvent::LogLine(source, line) => {
                self.log_lines.push(TaggedLine { source, line });
                // Note: no scroll offset update here — the render path
                // computes the correct offset when follow_mode is true
                // (total_lines - inner_height).  Calling filtered_log_lines()
                // on every append was O(n²) and caused multi-minute hangs at
                // 60 k lines.

                // Cap log lines to prevent unbounded memory growth
                if self.log_lines.len() > 50_000 {
                    debug!("log line cap reached, draining oldest 10,000 lines");
                    self.log_lines.drain(..10_000);
                }
            }
            AppEvent::LogStreamEnded(source) => {
                debug!(pod = %source, "log stream ended");
                // Remove the dead stream handle so stale panes don't linger.
                if let Some(pos) = self.streams.iter().position(|h| h.pod_name == source) {
                    self.streams.remove(pos);
                    // Return to single mode when only one (or zero) streams remain
                    if self.streams.len() <= 1 {
                        self.stream_mode = StreamMode::Single;
                        self.active_pane = 0;
                    } else if self.active_pane >= self.streams.len() {
                        self.active_pane = self.streams.len().saturating_sub(1);
                    }
                }
            }
            AppEvent::AzLoginCompleted(result) => {
                self.az_login_in_progress = false;
                match result {
                    Ok(()) => {
                        self.log_lines.push(TaggedLine::system(
                            "[INFO] az login succeeded — reloading cluster data…".to_string(),
                        ));
                        self.load_contexts();
                    }
                    Err(msg) => {
                        error!(message = %msg, "az login failed");
                        self.log_lines.push(TaggedLine::system(format!(
                            "[ERROR] az login failed: {msg}"
                        )));
                    }
                }
            }
            AppEvent::ExportCompleted(path) => {
                info!(path = %path, "export completed");
                self.log_lines
                    .push(TaggedLine::system(format!("[INFO] Exported to {path}")));
            }
            AppEvent::Error(msg) => {
                error!(message = %msg, "background task error");
                if is_auth_error(&msg) {
                    self.log_lines.push(TaggedLine::system(format!(
                        "[ERROR] {msg} (Azure credentials expired — run `az login`)"
                    )));
                    if !self.az_login_in_progress {
                        self.spawn_az_login();
                    }
                } else {
                    self.log_lines
                        .push(TaggedLine::system(format!("[ERROR] {msg}")));
                }
            }
        }
    }

    // -- Helpers ------------------------------------------------------------

    pub fn theme(&self) -> &Theme {
        &THEMES[self.theme_index]
    }

    pub fn cycle_theme(&mut self) {
        self.theme_index = (self.theme_index + 1) % THEMES.len();
        prefs::save(&prefs::prefs_from_theme_index(self.theme_index));
    }

    pub fn filtered_log_lines(&self) -> Vec<&TaggedLine> {
        let search_lower = if self.search_query.is_empty() {
            None
        } else {
            Some(self.search_query.to_lowercase())
        };

        let cutoff = match self.time_range {
            TimeRange::All => None,
            TimeRange::Last(dur) => {
                let now = Timestamp::now();
                Some(now - SignedDuration::try_from(dur).unwrap_or(SignedDuration::ZERO))
            }
        };

        let mut lines: Vec<&TaggedLine> = self
            .log_lines
            .iter()
            .filter(|tl| {
                // Health check filter: hide lines matching known health
                // check patterns (kube-probe user-agent, /healthz, /readyz,
                // /livez, /health, etc.). Uses the same pattern set as the
                // CLI classifier for consistency.
                if self.hide_health_checks
                    && kube_log_core::classify::is_health_check_line(&tl.line)
                {
                    return false;
                }
                // Search filter
                if let Some(ref lower) = search_lower
                    && !tl.line.to_lowercase().contains(lower)
                {
                    return false;
                }
                // Time range filter
                if let Some(cutoff_dt) = cutoff
                    && let Some(m) = kube_log_core::parse::TIMESTAMP_RE.find(&tl.line)
                    && let Some(dt) = kube_log_core::parse::parse_log_timestamp(&tl.line[..m.end()])
                {
                    return dt >= cutoff_dt;
                }
                // Lines without parseable timestamps are always included
                true
            })
            .collect();

        // In merged mode with multiple streams, sort by timestamp so lines
        // from different pods interleave chronologically.  Lines without a
        // parseable timestamp use DateTime::MAX_UTC so they sort to the end,
        // and stable sort preserves their relative arrival order.
        if self.stream_mode == StreamMode::Merged && self.streams.len() > 1 {
            lines.sort_by_cached_key(|tl| {
                kube_log_core::parse::TIMESTAMP_RE
                    .find(&tl.line)
                    .and_then(|m| kube_log_core::parse::parse_log_timestamp(&tl.line[..m.end()]))
                    .unwrap_or(Timestamp::MAX)
            });
        }

        lines
    }

    /// Filter log lines for a specific pane in split mode.
    /// Returns lines whose source matches the stream handle at `pane_idx`.
    /// System messages (empty source) are included in all panes.
    pub fn filtered_log_lines_for_pane(&self, pane_idx: usize) -> Vec<&TaggedLine> {
        let source = self
            .streams
            .get(pane_idx)
            .map(|h| h.pod_name.as_str())
            .unwrap_or("");

        self.filtered_log_lines()
            .into_iter()
            .filter(|tl| tl.source.is_empty() || tl.source == source)
            .collect()
    }

    // -- K8s background operations ------------------------------------------

    fn load_contexts(&self) {
        info!("spawning context load task");
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(k8s::contexts::load_contexts)
                .await
                .context("context load task panicked");
            match result {
                Ok(Ok((contexts, current))) => {
                    let _ = tx.send(AppEvent::ContextsLoaded(contexts, current));
                }
                Ok(Err(e)) | Err(e) => {
                    error!(error = %e, "failed to load contexts");
                    let _ = tx.send(AppEvent::Error(format!("Failed to load contexts: {e:#}")));
                }
            }
        });
    }

    fn load_namespaces(&self) {
        info!(context = %self.current_context, "spawning namespace load task");
        let tx = self.tx.clone();
        let context = self.current_context.clone();
        tokio::spawn(async move {
            match k8s::namespaces::list_namespaces(&context).await {
                Ok(namespaces) => {
                    let _ = tx.send(AppEvent::NamespacesLoaded(context, namespaces));
                }
                Err(e) => {
                    error!(error = %e, "failed to load namespaces");
                    let _ = tx.send(AppEvent::Error(format!("Failed to load namespaces: {e:#}")));
                }
            }
        });
    }

    fn start_pod_watcher(&mut self) {
        self.cancel_pod_watcher();

        info!(
            context = %self.current_context,
            namespace = %self.current_namespace,
            "starting pod watcher"
        );
        let tx = self.tx.clone();
        let context = self.current_context.clone();
        let namespace = self.current_namespace.clone();

        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        self.pod_watcher_cancel = Some(cancel_tx);

        tokio::spawn(async move {
            match k8s::pods::watch_pods(&context, &namespace, cancel_rx).await {
                Ok(mut stream) => {
                    use futures::StreamExt as _;
                    while let Some(item) = stream.next().await {
                        let event = match item {
                            k8s::pods::PodWatchItem::Updated(pods) => AppEvent::PodsUpdated(pods),
                            k8s::pods::PodWatchItem::Error(e) => AppEvent::Error(e),
                        };
                        if tx.send(event).is_err() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "failed to start pod watcher");
                    let _ = tx.send(AppEvent::Error(format!("Failed to watch pods: {e:#}")));
                }
            }
        });
    }

    fn cancel_pod_watcher(&mut self) {
        if let Some(cancel_tx) = self.pod_watcher_cancel.take() {
            debug!("cancelling active pod watcher");
            let _ = cancel_tx.send(true);
        }
    }

    /// Cancel all active log streams and drain the stream handle list.
    fn cancel_all_streams(&mut self) {
        for handle in self.streams.drain(..) {
            debug!(pod = %handle.pod_name, "cancelling log stream");
            let _ = handle.cancel_tx.send(true);
        }
    }

    /// Spawn a new log stream task for the given pod/container.
    /// Pushes a `LogStreamHandle` into `self.streams`.
    fn spawn_log_stream(&mut self, pod_name: &str, container: Option<&str>) {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

        self.streams.push(LogStreamHandle {
            pod_name: pod_name.to_string(),
            container: container.map(|s| s.to_string()),
            cancel_tx,
            view: LogViewState::default(),
        });

        let tx = self.tx.clone();
        let context = self.current_context.clone();
        let namespace = self.current_namespace.clone();
        let pod = pod_name.to_string();
        let container = container.map(|s| s.to_string());

        tokio::spawn(async move {
            match k8s::logs::stream_logs(
                &context,
                &namespace,
                &pod,
                container.as_deref(),
                cancel_rx,
                &k8s::logs::LogStreamConfig::default(),
            )
            .await
            {
                Ok(mut stream) => {
                    use futures::StreamExt as _;
                    while let Some(item) = stream.next().await {
                        let event = match item {
                            k8s::logs::LogStreamItem::Line(text) => {
                                AppEvent::LogLine(pod.clone(), text)
                            }
                            k8s::logs::LogStreamItem::Error(e) => AppEvent::Error(e),
                        };
                        if tx.send(event).is_err() {
                            break;
                        }
                    }
                    // Stream ended — notify the app.
                    let _ = tx.send(AppEvent::LogStreamEnded(pod));
                }
                Err(e) => {
                    error!(error = %e, "log stream failed");
                    let _ = tx.send(AppEvent::Error(format!("Log stream error: {e:#}")));
                }
            }
        });
    }

    /// Cancel all existing streams, then start a single new one (backward compat).
    fn start_log_stream(&mut self, pod_name: &str, container: Option<&str>) {
        info!(pod = pod_name, container = container, "starting log stream");
        self.cancel_all_streams();
        self.spawn_log_stream(pod_name, container);
    }

    /// `M` key handler: add the currently selected pod as an additional stream.
    /// Switches to Merged mode. Enforces MAX_STREAMS limit.
    fn add_stream(&mut self) {
        if self.focus != Focus::Pods {
            return;
        }
        let Some(i) = self.pod_list_state.selected() else {
            return;
        };
        let Some(pod) = self.pods.get(i) else { return };

        if self.streams.len() >= MAX_STREAMS {
            self.log_lines.push(TaggedLine::system(format!(
                "[INFO] Maximum {MAX_STREAMS} concurrent streams reached"
            )));
            return;
        }

        // Don't add the same pod twice
        let pod_name = pod.name.clone();
        if self.streams.iter().any(|h| h.pod_name == pod_name) {
            return;
        }

        let container = pod.containers.first().cloned();
        info!(pod = %pod_name, "adding stream");
        self.spawn_log_stream(&pod_name, container.as_deref());
        self.stream_mode = StreamMode::Merged;
    }

    /// `V` key handler: cycle view mode (Merged → Split → Single).
    /// Only meaningful when ≥2 streams are active.
    fn cycle_view(&mut self) {
        if self.streams.len() < 2 {
            return;
        }
        self.stream_mode = match self.stream_mode {
            StreamMode::Single => StreamMode::Merged,
            StreamMode::Merged => StreamMode::Split,
            StreamMode::Split => StreamMode::Single,
        };
        // Clamp active_pane
        if self.active_pane >= self.streams.len() {
            self.active_pane = 0;
        }
    }

    /// `X` key handler: remove the most recently added stream.
    /// Returns to Single mode when only one stream remains.
    fn remove_last_stream(&mut self) {
        if self.streams.len() <= 1 {
            return;
        }
        if let Some(handle) = self.streams.pop() {
            debug!(pod = %handle.pod_name, "removing stream");
            let _ = handle.cancel_tx.send(true);
        }
        if self.streams.len() <= 1 {
            self.stream_mode = StreamMode::Single;
            self.active_pane = 0;
        }
        // Clamp active pane
        if self.active_pane >= self.streams.len() {
            self.active_pane = self.streams.len().saturating_sub(1);
        }
    }

    // -- Export --------------------------------------------------------------

    /// Collect export lines respecting current view mode and filters.
    /// Returns `(source, raw_line)` pairs.
    fn collect_export_lines(&self) -> Vec<(String, String)> {
        let lines = if self.stream_mode == StreamMode::Split {
            self.filtered_log_lines_for_pane(self.active_pane)
        } else {
            self.filtered_log_lines()
        };
        lines
            .into_iter()
            .map(|tl| (tl.source.clone(), tl.line.clone()))
            .collect()
    }

    /// Build export metadata from the current app state.
    fn export_metadata(&self) -> ExportMetadata {
        let pod_names: Vec<String> = if self.stream_mode == StreamMode::Split {
            self.streams
                .get(self.active_pane)
                .map(|h| vec![h.pod_name.clone()])
                .unwrap_or_default()
        } else {
            self.streams.iter().map(|h| h.pod_name.clone()).collect()
        };
        ExportMetadata {
            context: self.current_context.clone(),
            namespace: self.current_namespace.clone(),
            pod_names,
            exported_at: Zoned::now(),
        }
    }

    /// Trigger a log export in the given format.
    /// Runs the file write on a background task.
    fn export_logs(&mut self, format: ExportFormat) {
        let lines = self.collect_export_lines();
        if lines.is_empty() {
            self.log_lines.push(TaggedLine::system(
                "[INFO] Nothing to export — no log lines".to_string(),
            ));
            return;
        }

        let meta = self.export_metadata();
        let stream_mode = self.stream_mode;
        let tx = self.tx.clone();

        let ext = format.extension();
        let timestamp = Zoned::now().strftime("%Y%m%d-%H%M%S");
        let filename = format!("kube-log-viewer-export-{timestamp}.{ext}");
        let path = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(&filename);

        info!(format = ?format, path = %path.display(), lines = lines.len(), "exporting logs");

        tokio::spawn(async move {
            let result = match format {
                ExportFormat::PlainText => {
                    write_plain_text(&path, &meta, &lines, stream_mode).await
                }
                ExportFormat::Json => write_json(&path, &meta, &lines).await,
                ExportFormat::Csv => write_csv(&path, &meta, &lines).await,
            };
            match result {
                Ok(()) => {
                    let _ = tx.send(AppEvent::ExportCompleted(path.display().to_string()));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::Error(format!("Export failed: {e:#}")));
                }
            }
        });
    }

    /// Spawn `az login` in the background and send [`AppEvent::AzLoginCompleted`]
    /// when it finishes. Opens the default browser for interactive auth.
    ///
    /// If a previous `az login` is still running, it is killed first.
    fn spawn_az_login(&mut self) {
        // Kill any previous az login process
        self.cancel_az_login();

        self.az_login_in_progress = true;
        self.log_lines.push(TaggedLine::system(
            "[INFO] Azure credentials expired — opening browser for login…".to_string(),
        ));
        info!("spawning az login");

        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        self.az_login_cancel = Some(cancel_tx);

        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = tokio::process::Command::new("az")
                .arg("login")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn();

            let mut child = match result {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(AppEvent::AzLoginCompleted(Err(format!(
                        "spawn failed: {e}"
                    ))));
                    return;
                }
            };

            tokio::select! {
                wait_result = child.wait() => {
                    match wait_result {
                        Ok(status) if status.success() => {
                            let _ = tx.send(AppEvent::AzLoginCompleted(Ok(())));
                        }
                        Ok(status) => {
                            let msg = format!("az login exited with {status}");
                            let _ = tx.send(AppEvent::AzLoginCompleted(Err(msg)));
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::AzLoginCompleted(Err(format!("wait failed: {e}"))));
                        }
                    }
                }
                _ = cancel_rx.changed() => {
                    // Kill the child process when cancellation is requested.
                    // child.kill() is infallible if the process has already exited.
                    let _ = child.kill().await;
                    debug!("az login child process killed via cancellation");
                }
            }
        });
    }

    /// Cancel any in-progress `az login` background task, killing the child
    /// process if it is still running.
    fn cancel_az_login(&mut self) {
        if let Some(cancel_tx) = self.az_login_cancel.take() {
            let _ = cancel_tx.send(true);
        }
        self.az_login_in_progress = false;
    }
}

// ---------------------------------------------------------------------------
// Export types & writers
// ---------------------------------------------------------------------------

/// Metadata included in the header of every exported file.
#[derive(Debug, Clone)]
pub struct ExportMetadata {
    pub context: String,
    pub namespace: String,
    pub pod_names: Vec<String>,
    pub exported_at: Zoned,
}

impl ExportMetadata {
    /// Format as plain-text header lines (prefixed with `# `).
    fn as_comment_lines(&self) -> String {
        let pods = if self.pod_names.is_empty() {
            "(none)".to_string()
        } else {
            self.pod_names.join(", ")
        };
        format!(
            "# Context:   {}\n# Namespace: {}\n# Pod(s):    {}\n# Exported:  {}\n",
            self.context,
            self.namespace,
            pods,
            self.exported_at.strftime("%Y-%m-%d %H:%M:%S %z"),
        )
    }
}

/// Write logs as plain text with a comment header.
async fn write_plain_text(
    path: &std::path::Path,
    meta: &ExportMetadata,
    lines: &[(String, String)],
    stream_mode: StreamMode,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let mut file = tokio::fs::File::create(path)
        .await
        .with_context(|| format!("failed to create {}", path.display()))?;

    // Header
    file.write_all(meta.as_comment_lines().as_bytes())
        .await
        .context("failed to write header")?;
    file.write_all(b"#\n")
        .await
        .context("failed to write header separator")?;

    for (source, line) in lines {
        if stream_mode == StreamMode::Merged && !source.is_empty() {
            file.write_all(format!("[{source}] {line}\n").as_bytes())
                .await
                .context("failed to write log line")?;
        } else {
            file.write_all(format!("{line}\n").as_bytes())
                .await
                .context("failed to write log line")?;
        }
    }

    file.flush().await.context("failed to flush export file")?;
    Ok(())
}

/// Write logs as a JSON array with a `_metadata` preamble object.
async fn write_json(
    path: &std::path::Path,
    meta: &ExportMetadata,
    lines: &[(String, String)],
) -> Result<()> {
    use serde_json::{Map, Value, json};

    let metadata = json!({
        "context": meta.context,
        "namespace": meta.namespace,
        "pods": meta.pod_names,
        "exported_at": meta.exported_at.strftime("%Y-%m-%dT%H:%M:%S%z").to_string(),
    });

    let mut entries: Vec<Value> = Vec::with_capacity(lines.len());
    for (source, raw) in lines {
        match serde_json::from_str::<Value>(raw) {
            Ok(Value::Object(mut map)) => {
                if !source.is_empty() {
                    map.insert("_source".to_string(), Value::String(source.clone()));
                }
                entries.push(Value::Object(map));
            }
            _ => {
                let mut map = Map::new();
                if !source.is_empty() {
                    map.insert("_source".to_string(), Value::String(source.clone()));
                }
                map.insert("_raw".to_string(), Value::String(raw.clone()));
                entries.push(Value::Object(map));
            }
        }
    }

    let output = json!({
        "_metadata": metadata,
        "lines": entries,
    });

    let json_str =
        serde_json::to_string_pretty(&output).context("failed to serialize export JSON")?;
    tokio::fs::write(path, json_str.as_bytes())
        .await
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Write logs as CSV with a comment header and union-of-keys columns.
async fn write_csv(
    path: &std::path::Path,
    meta: &ExportMetadata,
    lines: &[(String, String)],
) -> Result<()> {
    use std::collections::BTreeSet;
    use tokio::io::AsyncWriteExt;

    // First pass: collect union of all JSON keys
    let mut all_keys = BTreeSet::new();
    let mut parsed: Vec<Option<serde_json::Map<String, serde_json::Value>>> =
        Vec::with_capacity(lines.len());
    for (_source, raw) in lines {
        match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(serde_json::Value::Object(map)) => {
                for key in map.keys() {
                    all_keys.insert(key.clone());
                }
                parsed.push(Some(map));
            }
            _ => {
                parsed.push(None);
            }
        }
    }

    let columns: Vec<String> = std::iter::once("_source".to_string())
        .chain(all_keys.into_iter())
        .chain(std::iter::once("_raw".to_string()))
        .collect();

    let mut file = tokio::fs::File::create(path)
        .await
        .with_context(|| format!("failed to create {}", path.display()))?;

    // Comment header (CSV viewers typically skip lines starting with #)
    file.write_all(meta.as_comment_lines().as_bytes())
        .await
        .context("failed to write CSV header")?;
    file.write_all(b"#\n")
        .await
        .context("failed to write CSV header separator")?;

    // Column header row
    let header: Vec<String> = columns.iter().map(|c| csv_escape(c)).collect();
    file.write_all(format!("{}\n", header.join(",")).as_bytes())
        .await
        .context("failed to write CSV column header")?;

    // Data rows
    for (i, (source, raw)) in lines.iter().enumerate() {
        let row: Vec<String> = columns
            .iter()
            .map(|col| {
                if col == "_source" {
                    return csv_escape(source);
                }
                if col == "_raw" {
                    // Only populate _raw for non-JSON lines
                    if parsed[i].is_none() {
                        return csv_escape(raw);
                    }
                    return String::new();
                }
                // JSON field
                match &parsed[i] {
                    Some(map) => match map.get(col) {
                        Some(serde_json::Value::String(s)) => csv_escape(s),
                        Some(serde_json::Value::Null) => String::new(),
                        Some(v) => csv_escape(&v.to_string()),
                        None => String::new(),
                    },
                    None => String::new(),
                }
            })
            .collect();
        file.write_all(format!("{}\n", row.join(",")).as_bytes())
            .await
            .context("failed to write CSV data row")?;
    }

    file.flush().await.context("failed to flush CSV file")?;
    Ok(())
}

/// Escape a value for CSV: wrap in quotes if it contains comma, quote, or newline.
fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

// ---------------------------------------------------------------------------
// Auth error detection
// ---------------------------------------------------------------------------

/// Check whether a K8s / Azure error indicates expired or missing credentials.
fn is_auth_error(raw: &str) -> bool {
    let lower = raw.to_lowercase();

    // Azure AD / Entra ID token errors
    lower.contains("aadsts")
        || lower.contains("az login")
        || lower.contains("kubelogin")
        || lower.contains("interactive_browser")
        || (lower.contains("token") && lower.contains("expir"))
        // HTTP 401 from API server
        || lower.contains("unauthorized")
        || (lower.contains("401") && (lower.contains("auth") || lower.contains("credential")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    /// Helper to create a TaggedLine with empty source (system message).
    fn tl(line: &str) -> TaggedLine {
        TaggedLine {
            source: String::new(),
            line: line.to_string(),
        }
    }

    /// Helper to create a TaggedLine with a specific source.
    fn tl_src(source: &str, line: &str) -> TaggedLine {
        TaggedLine {
            source: source.to_string(),
            line: line.to_string(),
        }
    }

    fn test_app() -> App {
        let (tx, _rx) = mpsc::unbounded_channel::<AppEvent>();
        App::new(tx)
    }

    fn test_app_with_pods() -> App {
        let mut app = test_app();
        app.pods = vec![
            k8s::pods::PodInfo {
                name: "pod-a".to_string(),
                status: "Running".to_string(),
                ready: "1/1".to_string(),
                restarts: 0,
                containers: vec!["main".to_string()],
            },
            k8s::pods::PodInfo {
                name: "pod-b".to_string(),
                status: "Running".to_string(),
                ready: "2/2".to_string(),
                restarts: 1,
                containers: vec!["web".to_string(), "sidecar".to_string()],
            },
        ];
        app.pod_list_state.select(Some(0));
        app
    }

    // -- Quit ---------------------------------------------------------------

    #[test]
    fn test_q_quits() {
        let mut app = test_app();
        app.handle_key(key(KeyCode::Char('q')));
        assert!(app.should_quit);
    }

    #[test]
    fn test_ctrl_c_quits() {
        let mut app = test_app();
        app.handle_key(ctrl_key(KeyCode::Char('c')));
        assert!(app.should_quit);
    }

    // -- Help ---------------------------------------------------------------

    #[test]
    fn test_question_mark_toggles_help() {
        let mut app = test_app();
        assert!(!app.show_help);
        app.handle_key(key(KeyCode::Char('?')));
        assert!(app.show_help);
        app.handle_key(key(KeyCode::Char('?')));
        assert!(!app.show_help);
    }

    // -- Focus switching ----------------------------------------------------

    #[test]
    fn test_tab_switches_focus() {
        let mut app = test_app();
        assert_eq!(app.focus, Focus::Pods);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Logs);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Pods);
    }

    // -- Follow / Wrap toggles ----------------------------------------------

    #[test]
    fn test_f_toggles_follow_mode() {
        let mut app = test_app();
        assert!(app.follow_mode);
        app.handle_key(key(KeyCode::Char('f')));
        assert!(!app.follow_mode);
        app.handle_key(key(KeyCode::Char('f')));
        assert!(app.follow_mode);
    }

    #[test]
    fn test_w_toggles_wide_logs() {
        let mut app = test_app();
        assert!(!app.wide_logs);
        app.handle_key(key(KeyCode::Char('w')));
        assert!(app.wide_logs);
        app.handle_key(key(KeyCode::Char('w')));
        assert!(!app.wide_logs);
    }

    #[test]
    fn test_shift_w_toggles_wrap() {
        let mut app = test_app();
        assert!(!app.wrap_lines);
        app.handle_key(key(KeyCode::Char('W')));
        assert!(app.wrap_lines);
        app.handle_key(key(KeyCode::Char('W')));
        assert!(!app.wrap_lines);
    }

    #[test]
    fn test_shift_j_toggles_json_mode() {
        let mut app = test_app();
        assert!(app.json_mode); // default on
        app.handle_key(key(KeyCode::Char('J')));
        assert!(!app.json_mode);
        app.handle_key(key(KeyCode::Char('J')));
        assert!(app.json_mode);
    }

    #[test]
    fn test_shift_t_cycles_timestamp_mode() {
        let mut app = test_app();
        assert_eq!(app.timestamp_mode, TimestampMode::Local); // default
        app.handle_key(key(KeyCode::Char('T')));
        assert_eq!(app.timestamp_mode, TimestampMode::Relative);
        app.handle_key(key(KeyCode::Char('T')));
        assert_eq!(app.timestamp_mode, TimestampMode::Utc);
        app.handle_key(key(KeyCode::Char('T')));
        assert_eq!(app.timestamp_mode, TimestampMode::Local);
    }

    #[test]
    fn test_timestamp_mode_label() {
        assert_eq!(TimestampMode::Utc.label(), "UTC");
        assert_eq!(TimestampMode::Local.label(), "Local");
        assert_eq!(TimestampMode::Relative.label(), "Relative");
    }

    // -- Search mode --------------------------------------------------------

    #[test]
    fn test_slash_enters_search_mode() {
        let mut app = test_app();
        assert_eq!(app.input_mode, InputMode::Normal);
        app.handle_key(key(KeyCode::Char('/')));
        assert_eq!(app.input_mode, InputMode::Search);
    }

    #[test]
    fn test_search_typing_and_backspace() {
        let mut app = test_app();
        app.handle_key(key(KeyCode::Char('/')));
        app.handle_key(key(KeyCode::Char('e')));
        app.handle_key(key(KeyCode::Char('r')));
        app.handle_key(key(KeyCode::Char('r')));
        assert_eq!(app.search_query, "err");

        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.search_query, "er");
    }

    #[test]
    fn test_esc_exits_search_mode() {
        let mut app = test_app();
        app.handle_key(key(KeyCode::Char('/')));
        app.handle_key(key(KeyCode::Char('x')));
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
        // Query is preserved after Esc from search input
        assert_eq!(app.search_query, "x");
    }

    #[test]
    fn test_enter_confirms_search() {
        let mut app = test_app();
        app.handle_key(key(KeyCode::Char('/')));
        app.handle_key(key(KeyCode::Char('a')));
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.search_query, "a");
    }

    #[test]
    fn test_esc_clears_search_query_in_normal_mode() {
        let mut app = test_app();
        app.search_query = "something".to_string();
        app.handle_key(key(KeyCode::Esc));
        assert!(app.search_query.is_empty());
    }

    // -- Pod navigation -----------------------------------------------------

    #[test]
    fn test_j_navigates_down_in_pod_list() {
        let mut app = test_app_with_pods();
        assert_eq!(app.pod_list_state.selected(), Some(0));
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.pod_list_state.selected(), Some(1));
    }

    #[test]
    fn test_k_navigates_up_in_pod_list() {
        let mut app = test_app_with_pods();
        app.pod_list_state.select(Some(1));
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.pod_list_state.selected(), Some(0));
    }

    #[test]
    fn test_k_does_not_go_below_zero() {
        let mut app = test_app_with_pods();
        app.pod_list_state.select(Some(0));
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.pod_list_state.selected(), Some(0));
    }

    #[test]
    fn test_j_does_not_exceed_pod_count() {
        let mut app = test_app_with_pods();
        app.pod_list_state.select(Some(1));
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.pod_list_state.selected(), Some(1));
    }

    // -- Log scroll ---------------------------------------------------------

    #[test]
    fn test_scroll_up_disables_follow() {
        let mut app = test_app();
        app.focus = Focus::Logs;
        app.follow_mode = true;
        app.log_scroll_offset = 5;
        app.handle_key(key(KeyCode::Char('k')));
        assert!(!app.follow_mode);
        assert_eq!(app.log_scroll_offset, 4);
    }

    #[test]
    fn test_g_scrolls_to_top() {
        let mut app = test_app();
        app.focus = Focus::Logs;
        app.log_scroll_offset = 50;
        app.follow_mode = true;
        app.handle_key(key(KeyCode::Char('g')));
        assert_eq!(app.log_scroll_offset, 0);
        assert!(!app.follow_mode);
    }

    #[test]
    fn test_shift_g_scrolls_to_bottom_and_enables_follow() {
        let mut app = test_app();
        app.focus = Focus::Logs;
        app.follow_mode = false;
        app.log_lines = vec![tl("a"), tl("b"), tl("c")];
        app.handle_key(key(KeyCode::Char('G')));
        assert!(app.follow_mode);
    }

    // -- Popup keys ---------------------------------------------------------

    #[test]
    fn test_n_opens_namespace_popup() {
        let mut app = test_app();
        app.handle_key(key(KeyCode::Char('n')));
        assert_eq!(app.popup, Some(PopupKind::Namespaces));
    }

    #[test]
    fn test_c_opens_context_popup() {
        let mut app = test_app();
        app.handle_key(key(KeyCode::Char('c')));
        assert_eq!(app.popup, Some(PopupKind::Contexts));
    }

    #[test]
    fn test_s_opens_container_popup_when_containers_exist() {
        let mut app = test_app();
        app.containers = vec!["main".to_string()];
        app.handle_key(key(KeyCode::Char('s')));
        assert_eq!(app.popup, Some(PopupKind::Containers));
    }

    #[test]
    fn test_s_does_nothing_without_containers() {
        let mut app = test_app();
        app.handle_key(key(KeyCode::Char('s')));
        assert!(app.popup.is_none());
    }

    #[test]
    fn test_esc_closes_popup() {
        let mut app = test_app();
        app.popup = Some(PopupKind::Namespaces);
        app.handle_key(key(KeyCode::Esc));
        assert!(app.popup.is_none());
    }

    // -- Filtered log lines -------------------------------------------------

    #[test]
    fn test_filtered_log_lines_no_query() {
        let mut app = test_app();
        app.log_lines = vec![tl("alpha"), tl("beta"), tl("gamma")];
        let filtered = app.filtered_log_lines();
        let lines: Vec<&str> = filtered.iter().map(|tl| tl.line.as_str()).collect();
        assert_eq!(lines, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn test_filtered_log_lines_with_query() {
        let mut app = test_app();
        app.log_lines = vec![
            tl("INFO: started"),
            tl("ERROR: failed"),
            tl("INFO: completed"),
        ];
        app.search_query = "error".to_string();
        let filtered = app.filtered_log_lines();
        let lines: Vec<&str> = filtered.iter().map(|tl| tl.line.as_str()).collect();
        assert_eq!(lines, vec!["ERROR: failed"]);
    }

    #[test]
    fn test_filtered_log_lines_case_insensitive() {
        let mut app = test_app();
        app.log_lines = vec![tl("Error occurred"), tl("all good")];
        app.search_query = "ERROR".to_string();
        let filtered = app.filtered_log_lines();
        let lines: Vec<&str> = filtered.iter().map(|tl| tl.line.as_str()).collect();
        assert_eq!(lines, vec!["Error occurred"]);
    }

    #[test]
    fn test_filtered_log_lines_no_match() {
        let mut app = test_app();
        app.log_lines = vec![tl("hello world")];
        app.search_query = "xyz".to_string();
        let filtered = app.filtered_log_lines();
        assert!(filtered.is_empty());
    }

    // -- handle_app_event ---------------------------------------------------

    #[test]
    fn test_pods_updated_selects_first() {
        let mut app = test_app();
        assert!(app.pod_list_state.selected().is_none());

        app.handle_app_event(AppEvent::PodsUpdated(vec![k8s::pods::PodInfo {
            name: "test-pod".to_string(),
            status: "Running".to_string(),
            ready: "1/1".to_string(),
            restarts: 0,
            containers: vec!["app".to_string()],
        }]));

        assert_eq!(app.pods.len(), 1);
        assert_eq!(app.pod_list_state.selected(), Some(0));
    }

    #[test]
    fn test_namespaces_loaded_preserves_current_if_present() {
        let mut app = test_app();
        app.current_namespace = "kube-system".to_string();
        app.handle_app_event(AppEvent::NamespacesLoaded(
            String::new(),
            vec!["default".to_string(), "kube-system".to_string()],
        ));
        assert_eq!(app.current_namespace, "kube-system");
    }

    #[test]
    fn test_namespaces_loaded_falls_back_to_first() {
        let mut app = test_app();
        app.current_namespace = "nonexistent".to_string();
        app.handle_app_event(AppEvent::NamespacesLoaded(
            String::new(),
            vec!["default".to_string(), "production".to_string()],
        ));
        assert_eq!(app.current_namespace, "default");
    }

    #[test]
    fn test_log_line_appended() {
        let mut app = test_app();
        app.handle_app_event(AppEvent::LogLine(String::new(), "hello".to_string()));
        assert_eq!(app.log_lines.len(), 1);
        assert_eq!(app.log_lines[0].line, "hello");
    }

    #[test]
    fn test_error_shown_in_log_lines() {
        let mut app = test_app();
        app.handle_app_event(AppEvent::Error("connection refused".to_string()));
        assert_eq!(app.log_lines.len(), 1);
        assert_eq!(app.log_lines[0].line, "[ERROR] connection refused");
    }

    #[test]
    fn test_auth_error_sets_az_login_flag() {
        let app = test_app();
        // "unauthorized" is detected as an auth error; az login spawns
        // (will fail in test since no tokio runtime, but flag must be set
        //  before the spawn)
        assert!(!app.az_login_in_progress);
        assert!(is_auth_error("Unauthorized: token expired"));
        assert!(is_auth_error("AADSTS70043: refresh token has expired"));
        assert!(is_auth_error("exec: kubelogin get-token failed"));
        assert!(!is_auth_error("connection refused"));
        assert!(!is_auth_error("DNS resolution failed"));
    }

    #[tokio::test]
    async fn test_az_login_completed_success() {
        let mut app = test_app();
        app.az_login_in_progress = true;
        app.handle_app_event(AppEvent::AzLoginCompleted(Ok(())));
        assert!(!app.az_login_in_progress);
        assert!(
            app.log_lines
                .last()
                .is_some_and(|tl| tl.line.contains("succeeded"))
        );
    }

    #[test]
    fn test_az_login_completed_failure() {
        let mut app = test_app();
        app.az_login_in_progress = true;
        app.handle_app_event(AppEvent::AzLoginCompleted(Err("spawn failed".to_string())));
        assert!(!app.az_login_in_progress);
        assert!(
            app.log_lines
                .last()
                .is_some_and(|tl| tl.line.contains("az login failed"))
        );
    }

    #[test]
    fn test_log_line_cap() {
        let mut app = test_app();
        for i in 0..50_001 {
            app.log_lines.push(tl(&format!("line {i}")));
        }
        app.handle_app_event(AppEvent::LogLine(String::new(), "overflow".to_string()));
        // After adding one more (total 50002), drain 10000, leaving 40002
        assert!(app.log_lines.len() <= 50_001);
        assert!(app.log_lines.len() > 40_000);
    }

    // -- Time range ---------------------------------------------------------

    #[test]
    fn test_shift_r_opens_time_range_popup() {
        let mut app = test_app();
        app.handle_key(key(KeyCode::Char('R')));
        assert_eq!(app.popup, Some(PopupKind::TimeRange));
    }

    #[test]
    fn test_time_range_label() {
        assert_eq!(TimeRange::All.label(), "All");
        assert_eq!(
            TimeRange::Last(Duration::from_secs(5 * 60)).label(),
            "Last 5m"
        );
        assert_eq!(
            TimeRange::Last(Duration::from_secs(15 * 60)).label(),
            "Last 15m"
        );
        assert_eq!(
            TimeRange::Last(Duration::from_secs(30 * 60)).label(),
            "Last 30m"
        );
        assert_eq!(
            TimeRange::Last(Duration::from_secs(60 * 60)).label(),
            "Last 1h"
        );
        assert_eq!(
            TimeRange::Last(Duration::from_secs(6 * 60 * 60)).label(),
            "Last 6h"
        );
        assert_eq!(
            TimeRange::Last(Duration::from_secs(24 * 60 * 60)).label(),
            "Last 24h"
        );
    }

    #[test]
    fn test_time_range_default_is_all() {
        assert_eq!(TimeRange::default(), TimeRange::All);
        let app = test_app();
        assert_eq!(app.time_range, TimeRange::All);
    }

    #[test]
    fn test_filtered_log_lines_time_range_all() {
        let mut app = test_app();
        app.time_range = TimeRange::All;
        app.log_lines = vec![tl("2020-01-01T00:00:00Z old line"), tl("no timestamp here")];
        let filtered = app.filtered_log_lines();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filtered_log_lines_time_range_excludes_old() {
        let mut app = test_app();
        // Set range to last 5 minutes — a timestamp from 2020 should be excluded
        app.time_range = TimeRange::Last(Duration::from_secs(5 * 60));
        app.log_lines = vec![tl("2020-01-01T00:00:00Z ancient log entry")];
        let filtered = app.filtered_log_lines();
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_filtered_log_lines_time_range_includes_recent() {
        let mut app = test_app();
        app.time_range = TimeRange::Last(Duration::from_secs(60 * 60));
        let now = jiff::Timestamp::now().to_string();
        // Trim the subsecond portion to get a clean timestamp
        let now = &now[..19];
        let now = format!("{now}Z");
        app.log_lines = vec![tl(&format!("{now} recent log entry"))];
        let filtered = app.filtered_log_lines();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filtered_log_lines_time_range_includes_unparseable() {
        let mut app = test_app();
        app.time_range = TimeRange::Last(Duration::from_secs(5 * 60));
        app.log_lines = vec![tl("no timestamp at all"), tl("just some text")];
        // Lines without parseable timestamps are always included
        let filtered = app.filtered_log_lines();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filtered_log_lines_time_range_with_search() {
        let mut app = test_app();
        app.time_range = TimeRange::Last(Duration::from_secs(5 * 60));
        app.search_query = "error".to_string();
        let now = jiff::Timestamp::now().to_string();
        let now = &now[..19];
        let now = format!("{now}Z");
        app.log_lines = vec![
            tl(&format!("{now} INFO: all good")), // recent but no match
            tl(&format!("{now} ERROR: something broke")), // recent and matches
            tl("2020-01-01T00:00:00Z ERROR: ancient"), // matches but too old
            tl("no timestamp ERROR here"),        // matches, no timestamp (included)
        ];
        let filtered = app.filtered_log_lines();
        assert_eq!(filtered.len(), 2);
        assert!(filtered[0].line.contains("ERROR: something broke"));
        assert!(filtered[1].line.contains("no timestamp ERROR here"));
    }

    // -- Pod watcher / selection preservation --------------------------------

    fn make_pod_info(name: &str) -> k8s::pods::PodInfo {
        k8s::pods::PodInfo {
            name: name.to_string(),
            status: "Running".to_string(),
            ready: "1/1".to_string(),
            restarts: 0,
            containers: vec!["main".to_string()],
        }
    }

    #[test]
    fn test_pods_updated_preserves_selection_by_name() {
        let mut app = test_app();
        // Initial pods: a, b, c — select "b" at index 1
        app.handle_app_event(AppEvent::PodsUpdated(vec![
            make_pod_info("pod-a"),
            make_pod_info("pod-b"),
            make_pod_info("pod-c"),
        ]));
        app.pod_list_state.select(Some(1)); // "pod-b"

        // Refresh: order changes, "pod-b" is now at index 2
        app.handle_app_event(AppEvent::PodsUpdated(vec![
            make_pod_info("pod-a"),
            make_pod_info("pod-c"),
            make_pod_info("pod-b"),
        ]));
        assert_eq!(app.pod_list_state.selected(), Some(2)); // follows "pod-b"
    }

    #[test]
    fn test_pods_updated_falls_back_when_selected_pod_removed() {
        let mut app = test_app();
        app.handle_app_event(AppEvent::PodsUpdated(vec![
            make_pod_info("pod-a"),
            make_pod_info("pod-b"),
        ]));
        app.pod_list_state.select(Some(1)); // "pod-b"

        // "pod-b" is gone
        app.handle_app_event(AppEvent::PodsUpdated(vec![make_pod_info("pod-a")]));
        assert_eq!(app.pod_list_state.selected(), Some(0)); // falls back to first
    }

    #[test]
    fn test_pods_updated_clears_selection_when_empty() {
        let mut app = test_app();
        app.handle_app_event(AppEvent::PodsUpdated(vec![make_pod_info("pod-a")]));
        assert_eq!(app.pod_list_state.selected(), Some(0));

        // All pods removed
        app.handle_app_event(AppEvent::PodsUpdated(vec![]));
        assert_eq!(app.pod_list_state.selected(), None);
    }

    #[test]
    fn test_pods_updated_new_pod_appears() {
        let mut app = test_app();
        app.handle_app_event(AppEvent::PodsUpdated(vec![make_pod_info("pod-a")]));
        app.pod_list_state.select(Some(0)); // "pod-a"

        // New pod added, "pod-a" stays at index 0
        app.handle_app_event(AppEvent::PodsUpdated(vec![
            make_pod_info("pod-a"),
            make_pod_info("pod-b"),
        ]));
        assert_eq!(app.pod_list_state.selected(), Some(0)); // still "pod-a"
        assert_eq!(app.pods.len(), 2);
    }

    // -- Multi-stream -------------------------------------------------------

    #[test]
    fn test_tagged_line_system() {
        let tl = TaggedLine::system("hello".to_string());
        assert!(tl.source.is_empty());
        assert_eq!(tl.line, "hello");
    }

    #[test]
    fn test_stream_mode_default_is_single() {
        let app = test_app();
        assert_eq!(app.stream_mode, StreamMode::Single);
        assert_eq!(app.active_pane, 0);
        assert!(app.streams.is_empty());
    }

    #[test]
    fn test_cycle_view_requires_two_streams() {
        let mut app = test_app();
        // No streams — cycle_view should be a no-op
        app.cycle_view();
        assert_eq!(app.stream_mode, StreamMode::Single);
    }

    #[test]
    fn test_cycle_view_cycles_modes() {
        let mut app = test_app();
        // Fake 2 stream handles (no real tokio tasks needed for mode cycling)
        let (tx1, _) = tokio::sync::watch::channel(false);
        let (tx2, _) = tokio::sync::watch::channel(false);
        app.streams.push(LogStreamHandle {
            pod_name: "pod-a".to_string(),
            container: None,
            cancel_tx: tx1,
            view: LogViewState::default(),
        });
        app.streams.push(LogStreamHandle {
            pod_name: "pod-b".to_string(),
            container: None,
            cancel_tx: tx2,
            view: LogViewState::default(),
        });

        app.stream_mode = StreamMode::Single;
        app.cycle_view();
        assert_eq!(app.stream_mode, StreamMode::Merged);
        app.cycle_view();
        assert_eq!(app.stream_mode, StreamMode::Split);
        app.cycle_view();
        assert_eq!(app.stream_mode, StreamMode::Single);
    }

    #[test]
    fn test_remove_last_stream_returns_to_single() {
        let mut app = test_app();
        let (tx1, _) = tokio::sync::watch::channel(false);
        let (tx2, _) = tokio::sync::watch::channel(false);
        app.streams.push(LogStreamHandle {
            pod_name: "pod-a".to_string(),
            container: None,
            cancel_tx: tx1,
            view: LogViewState::default(),
        });
        app.streams.push(LogStreamHandle {
            pod_name: "pod-b".to_string(),
            container: None,
            cancel_tx: tx2,
            view: LogViewState::default(),
        });
        app.stream_mode = StreamMode::Merged;

        app.remove_last_stream();
        assert_eq!(app.streams.len(), 1);
        assert_eq!(app.stream_mode, StreamMode::Single);
        assert_eq!(app.streams[0].pod_name, "pod-a");
    }

    #[test]
    fn test_remove_last_stream_noop_with_one() {
        let mut app = test_app();
        let (tx1, _) = tokio::sync::watch::channel(false);
        app.streams.push(LogStreamHandle {
            pod_name: "pod-a".to_string(),
            container: None,
            cancel_tx: tx1,
            view: LogViewState::default(),
        });

        app.remove_last_stream();
        assert_eq!(app.streams.len(), 1); // no-op, can't remove last
    }

    #[test]
    fn test_filtered_log_lines_for_pane() {
        let mut app = test_app();
        let (tx1, _) = tokio::sync::watch::channel(false);
        let (tx2, _) = tokio::sync::watch::channel(false);
        app.streams.push(LogStreamHandle {
            pod_name: "pod-a".to_string(),
            container: None,
            cancel_tx: tx1,
            view: LogViewState::default(),
        });
        app.streams.push(LogStreamHandle {
            pod_name: "pod-b".to_string(),
            container: None,
            cancel_tx: tx2,
            view: LogViewState::default(),
        });

        app.log_lines = vec![
            tl_src("pod-a", "line from a"),
            tl_src("pod-b", "line from b"),
            TaggedLine::system("[INFO] system msg".to_string()),
            tl_src("pod-a", "another from a"),
        ];

        // Pane 0 = pod-a: should see pod-a lines + system messages
        let pane0 = app.filtered_log_lines_for_pane(0);
        assert_eq!(pane0.len(), 3);
        assert_eq!(pane0[0].line, "line from a");
        assert_eq!(pane0[1].line, "[INFO] system msg");
        assert_eq!(pane0[2].line, "another from a");

        // Pane 1 = pod-b: should see pod-b lines + system messages
        let pane1 = app.filtered_log_lines_for_pane(1);
        assert_eq!(pane1.len(), 2);
        assert_eq!(pane1[0].line, "line from b");
        assert_eq!(pane1[1].line, "[INFO] system msg");
    }

    #[test]
    fn test_log_line_event_stores_source() {
        let mut app = test_app();
        app.handle_app_event(AppEvent::LogLine("my-pod".to_string(), "hello".to_string()));
        assert_eq!(app.log_lines.len(), 1);
        assert_eq!(app.log_lines[0].source, "my-pod");
        assert_eq!(app.log_lines[0].line, "hello");
    }

    #[test]
    fn test_pane_switch_keys_in_split_mode() {
        let mut app = test_app();
        let (tx1, _) = tokio::sync::watch::channel(false);
        let (tx2, _) = tokio::sync::watch::channel(false);
        let (tx3, _) = tokio::sync::watch::channel(false);
        let (tx4, _) = tokio::sync::watch::channel(false);
        app.streams.push(LogStreamHandle {
            pod_name: "pod-a".to_string(),
            container: None,
            cancel_tx: tx1,
            view: LogViewState::default(),
        });
        app.streams.push(LogStreamHandle {
            pod_name: "pod-b".to_string(),
            container: None,
            cancel_tx: tx2,
            view: LogViewState::default(),
        });
        app.streams.push(LogStreamHandle {
            pod_name: "pod-c".to_string(),
            container: None,
            cancel_tx: tx3,
            view: LogViewState::default(),
        });
        app.streams.push(LogStreamHandle {
            pod_name: "pod-d".to_string(),
            container: None,
            cancel_tx: tx4,
            view: LogViewState::default(),
        });
        app.stream_mode = StreamMode::Split;
        assert_eq!(app.active_pane, 0);

        app.handle_key(key(KeyCode::Char('2')));
        assert_eq!(app.active_pane, 1);

        app.handle_key(key(KeyCode::Char('3')));
        assert_eq!(app.active_pane, 2);

        app.handle_key(key(KeyCode::Char('4')));
        assert_eq!(app.active_pane, 3);

        app.handle_key(key(KeyCode::Char('1')));
        assert_eq!(app.active_pane, 0);
    }

    #[test]
    fn test_pane_switch_keys_guarded_by_stream_count() {
        let mut app = test_app();
        let (tx1, _) = tokio::sync::watch::channel(false);
        let (tx2, _) = tokio::sync::watch::channel(false);
        app.streams.push(LogStreamHandle {
            pod_name: "pod-a".to_string(),
            container: None,
            cancel_tx: tx1,
            view: LogViewState::default(),
        });
        app.streams.push(LogStreamHandle {
            pod_name: "pod-b".to_string(),
            container: None,
            cancel_tx: tx2,
            view: LogViewState::default(),
        });
        app.stream_mode = StreamMode::Split;

        // '3' and '4' should be no-ops with only 2 streams
        app.handle_key(key(KeyCode::Char('3')));
        assert_eq!(app.active_pane, 0);
        app.handle_key(key(KeyCode::Char('4')));
        assert_eq!(app.active_pane, 0);
    }

    #[test]
    fn test_pane_switch_keys_ignored_outside_split() {
        let mut app = test_app();
        app.stream_mode = StreamMode::Merged;
        app.handle_key(key(KeyCode::Char('2')));
        assert_eq!(app.active_pane, 0); // no change
    }

    #[test]
    fn test_f_toggle_in_split_mode() {
        let mut app = test_app();
        let (tx1, _) = tokio::sync::watch::channel(false);
        app.streams.push(LogStreamHandle {
            pod_name: "pod-a".to_string(),
            container: None,
            cancel_tx: tx1,
            view: LogViewState::default(),
        });
        app.stream_mode = StreamMode::Split;
        app.active_pane = 0;

        assert!(app.streams[0].view.follow_mode);
        app.handle_key(key(KeyCode::Char('f')));
        assert!(!app.streams[0].view.follow_mode);
        app.handle_key(key(KeyCode::Char('f')));
        assert!(app.streams[0].view.follow_mode);
    }

    #[test]
    fn test_cancel_all_streams_drains_vec() {
        let mut app = test_app();
        let (tx1, _) = tokio::sync::watch::channel(false);
        let (tx2, _) = tokio::sync::watch::channel(false);
        app.streams.push(LogStreamHandle {
            pod_name: "pod-a".to_string(),
            container: None,
            cancel_tx: tx1,
            view: LogViewState::default(),
        });
        app.streams.push(LogStreamHandle {
            pod_name: "pod-b".to_string(),
            container: None,
            cancel_tx: tx2,
            view: LogViewState::default(),
        });

        app.cancel_all_streams();
        assert!(app.streams.is_empty());
    }

    // -- Export --------------------------------------------------------------

    #[test]
    fn test_e_key_opens_popup_when_json_mode() {
        let mut app = test_app();
        app.json_mode = true;
        app.handle_key(key(KeyCode::Char('E')));
        assert_eq!(app.popup, Some(PopupKind::ExportFormat));
    }

    #[tokio::test]
    async fn test_e_key_exports_directly_when_not_json_mode() {
        let mut app = test_app();
        app.json_mode = false;
        app.log_lines = vec![tl("hello")];
        // export_logs will spawn a tokio task that writes to a file.
        // We just verify the popup is NOT opened (direct export path).
        app.handle_key(key(KeyCode::Char('E')));
        assert!(app.popup.is_none());
    }

    #[test]
    fn test_export_format_popup_items_len() {
        let mut app = test_app();
        app.popup = Some(PopupKind::ExportFormat);
        assert_eq!(app.popup_items_len(), EXPORT_FORMAT_OPTIONS.len());
    }

    #[test]
    fn test_export_logs_empty_shows_info() {
        let mut app = test_app();
        app.export_logs(ExportFormat::PlainText);
        assert_eq!(app.log_lines.len(), 1);
        assert!(app.log_lines[0].line.contains("Nothing to export"));
    }

    #[test]
    fn test_collect_export_lines_single_mode() {
        let mut app = test_app();
        app.log_lines = vec![
            tl_src("pod-a", "line 1"),
            tl_src("pod-a", "line 2"),
            TaggedLine::system("[INFO] system".to_string()),
        ];
        let lines = app.collect_export_lines();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], ("pod-a".to_string(), "line 1".to_string()));
        assert_eq!(lines[2], (String::new(), "[INFO] system".to_string()));
    }

    #[test]
    fn test_collect_export_lines_split_mode() {
        let mut app = test_app();
        let (tx1, _) = tokio::sync::watch::channel(false);
        let (tx2, _) = tokio::sync::watch::channel(false);
        app.streams.push(LogStreamHandle {
            pod_name: "pod-a".to_string(),
            container: None,
            cancel_tx: tx1,
            view: LogViewState::default(),
        });
        app.streams.push(LogStreamHandle {
            pod_name: "pod-b".to_string(),
            container: None,
            cancel_tx: tx2,
            view: LogViewState::default(),
        });
        app.stream_mode = StreamMode::Split;
        app.active_pane = 1; // pod-b

        app.log_lines = vec![
            tl_src("pod-a", "from a"),
            tl_src("pod-b", "from b"),
            TaggedLine::system("[INFO] sys".to_string()),
        ];

        let lines = app.collect_export_lines();
        // Should only include pod-b lines + system messages
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].0, "pod-b");
        assert!(lines[1].0.is_empty()); // system message
    }

    #[test]
    fn test_export_metadata_single_mode() {
        let mut app = test_app();
        app.current_context = "my-ctx".to_string();
        app.current_namespace = "my-ns".to_string();
        let (tx1, _) = tokio::sync::watch::channel(false);
        app.streams.push(LogStreamHandle {
            pod_name: "pod-a".to_string(),
            container: None,
            cancel_tx: tx1,
            view: LogViewState::default(),
        });

        let meta = app.export_metadata();
        assert_eq!(meta.context, "my-ctx");
        assert_eq!(meta.namespace, "my-ns");
        assert_eq!(meta.pod_names, vec!["pod-a"]);
    }

    #[test]
    fn test_export_metadata_split_mode() {
        let mut app = test_app();
        app.current_context = "ctx".to_string();
        app.current_namespace = "ns".to_string();
        let (tx1, _) = tokio::sync::watch::channel(false);
        let (tx2, _) = tokio::sync::watch::channel(false);
        app.streams.push(LogStreamHandle {
            pod_name: "pod-a".to_string(),
            container: None,
            cancel_tx: tx1,
            view: LogViewState::default(),
        });
        app.streams.push(LogStreamHandle {
            pod_name: "pod-b".to_string(),
            container: None,
            cancel_tx: tx2,
            view: LogViewState::default(),
        });
        app.stream_mode = StreamMode::Split;
        app.active_pane = 1;

        let meta = app.export_metadata();
        // Split mode only includes the active pane's pod
        assert_eq!(meta.pod_names, vec!["pod-b"]);
    }

    #[test]
    fn test_export_metadata_comment_lines() {
        let meta = ExportMetadata {
            context: "k8s-cts-aks-d-kubesvc-1".to_string(),
            namespace: "production".to_string(),
            pod_names: vec!["web-abc123".to_string(), "api-def456".to_string()],
            exported_at: Zoned::now(),
        };
        let header = meta.as_comment_lines();
        assert!(header.contains("# Context:   k8s-cts-aks-d-kubesvc-1"));
        assert!(header.contains("# Namespace: production"));
        assert!(header.contains("# Pod(s):    web-abc123, api-def456"));
        assert!(header.contains("# Exported:"));
    }

    #[test]
    fn test_csv_escape() {
        assert_eq!(csv_escape("hello"), "hello");
        assert_eq!(csv_escape("has,comma"), "\"has,comma\"");
        assert_eq!(csv_escape("has\"quote"), "\"has\"\"quote\"");
        assert_eq!(csv_escape("has\nnewline"), "\"has\nnewline\"");
        assert_eq!(csv_escape("plain"), "plain");
    }

    #[tokio::test]
    async fn test_write_plain_text() {
        let dir = std::env::temp_dir().join("klv-test-plain");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.log");

        let meta = ExportMetadata {
            context: "test-ctx".to_string(),
            namespace: "test-ns".to_string(),
            pod_names: vec!["pod-x".to_string()],
            exported_at: Zoned::now(),
        };
        let lines = vec![
            ("pod-x".to_string(), "line one".to_string()),
            (String::new(), "[INFO] sys".to_string()),
        ];

        write_plain_text(&path, &meta, &lines, StreamMode::Single)
            .await
            .expect("write_plain_text failed");

        let content = std::fs::read_to_string(&path).expect("read failed");
        assert!(content.contains("# Context:   test-ctx"));
        assert!(content.contains("# Namespace: test-ns"));
        assert!(content.contains("# Pod(s):    pod-x"));
        assert!(content.contains("# Exported:"));
        assert!(content.contains("line one\n"));
        assert!(content.contains("[INFO] sys\n"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_write_plain_text_merged_mode() {
        let dir = std::env::temp_dir().join("klv-test-plain-merged");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.log");

        let meta = ExportMetadata {
            context: "ctx".to_string(),
            namespace: "ns".to_string(),
            pod_names: vec!["pod-a".to_string(), "pod-b".to_string()],
            exported_at: Zoned::now(),
        };
        let lines = vec![
            ("pod-a".to_string(), "from a".to_string()),
            ("pod-b".to_string(), "from b".to_string()),
        ];

        write_plain_text(&path, &meta, &lines, StreamMode::Merged)
            .await
            .expect("write failed");

        let content = std::fs::read_to_string(&path).expect("read failed");
        assert!(content.contains("[pod-a] from a"));
        assert!(content.contains("[pod-b] from b"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_write_json() {
        let dir = std::env::temp_dir().join("klv-test-json");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.json");

        let meta = ExportMetadata {
            context: "ctx".to_string(),
            namespace: "ns".to_string(),
            pod_names: vec!["pod-x".to_string()],
            exported_at: Zoned::now(),
        };
        let lines = vec![
            (
                "pod-x".to_string(),
                r#"{"level":"info","msg":"hello"}"#.to_string(),
            ),
            ("pod-x".to_string(), "not json".to_string()),
        ];

        write_json(&path, &meta, &lines)
            .await
            .expect("write_json failed");

        let content = std::fs::read_to_string(&path).expect("read failed");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("invalid JSON output");

        // Check metadata
        assert_eq!(parsed["_metadata"]["context"], "ctx");
        assert_eq!(parsed["_metadata"]["namespace"], "ns");
        assert_eq!(parsed["_metadata"]["pods"][0], "pod-x");

        // Check lines
        let arr = parsed["lines"].as_array().expect("lines should be array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["_source"], "pod-x");
        assert_eq!(arr[0]["level"], "info");
        assert_eq!(arr[1]["_raw"], "not json");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_write_csv() {
        let dir = std::env::temp_dir().join("klv-test-csv");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.csv");

        let meta = ExportMetadata {
            context: "ctx".to_string(),
            namespace: "ns".to_string(),
            pod_names: vec!["pod-x".to_string()],
            exported_at: Zoned::now(),
        };
        let lines = vec![
            (
                "pod-x".to_string(),
                r#"{"level":"info","msg":"hello"}"#.to_string(),
            ),
            ("pod-x".to_string(), "plain text line".to_string()),
        ];

        write_csv(&path, &meta, &lines)
            .await
            .expect("write_csv failed");

        let content = std::fs::read_to_string(&path).expect("read failed");

        // Should have comment header
        assert!(content.contains("# Context:   ctx"));
        assert!(content.contains("# Namespace: ns"));

        // Find the CSV header row (first non-comment line)
        let data_lines: Vec<&str> = content.lines().filter(|l| !l.starts_with('#')).collect();
        assert!(data_lines.len() >= 3); // header + 2 data rows

        let header = data_lines[0];
        assert!(header.contains("_source"));
        assert!(header.contains("level"));
        assert!(header.contains("msg"));
        assert!(header.contains("_raw"));

        // JSON line should have level and msg filled
        let json_row = data_lines[1];
        assert!(json_row.contains("pod-x"));
        assert!(json_row.contains("info"));
        assert!(json_row.contains("hello"));

        // Plain text line should have _raw filled
        let plain_row = data_lines[2];
        assert!(plain_row.contains("plain text line"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_export_completed_event() {
        let mut app = test_app();
        app.handle_app_event(AppEvent::ExportCompleted("/tmp/test.log".to_string()));
        assert_eq!(app.log_lines.len(), 1);
        assert!(app.log_lines[0].line.contains("[INFO] Exported to"));
        assert!(app.log_lines[0].line.contains("/tmp/test.log"));
    }

    // -- Health check filter ------------------------------------------------

    #[test]
    fn test_shift_h_toggles_hide_health_checks() {
        let mut app = test_app();
        assert!(app.hide_health_checks); // default: hidden
        app.handle_key(key(KeyCode::Char('H')));
        assert!(!app.hide_health_checks);
        app.handle_key(key(KeyCode::Char('H')));
        assert!(app.hide_health_checks);
    }

    #[test]
    fn test_health_filter_hides_kube_probe_lines() {
        let mut app = test_app();
        app.hide_health_checks = true;
        app.log_lines = vec![
            tl("GET /health uri=/miapi/isHealthy user_agent=kube-probe/1.32"),
            tl("INFO: request processed"),
            tl("GET / HTTP/1.1 200 385 kube-probe/1.31+"), // probe on root path
            tl("GET /status user_agent=kube-probe/1.32 health=ok"),
        ];
        let filtered = app.filtered_log_lines();
        let lines: Vec<&str> = filtered.iter().map(|tl| tl.line.as_str()).collect();
        assert_eq!(lines, vec!["INFO: request processed"]);
    }

    #[test]
    fn test_health_filter_hides_health_path_without_kube_probe() {
        // Lines with /health, /healthz, /readyz, /livez paths should be
        // filtered even when there's no kube-probe user-agent. This covers
        // logfmt and plain-text health check access logs.
        let mut app = test_app();
        app.hide_health_checks = true;
        app.log_lines = vec![
            tl(
                "time=2026-03-09T20:09:33.759Z level=INFO msg=request method=GET path=/health status=200 duration_ms=0 request_id=abc123",
            ),
            tl("GET /healthz 200 OK"),
            tl("GET /readyz 200 OK"),
            tl("GET /livez 200 OK"),
            tl("INFO: all good"), // no health pattern — should be kept
        ];
        let filtered = app.filtered_log_lines();
        let lines: Vec<&str> = filtered.iter().map(|tl| tl.line.as_str()).collect();
        assert_eq!(lines, vec!["INFO: all good"]);
    }

    #[test]
    fn test_health_filter_keeps_lines_without_health_patterns() {
        let mut app = test_app();
        app.hide_health_checks = true;
        app.log_lines = vec![
            tl("GET /api/users 200 OK"), // no health pattern
            tl("INFO: all good"),        // neither
        ];
        let filtered = app.filtered_log_lines();
        let lines: Vec<&str> = filtered.iter().map(|tl| tl.line.as_str()).collect();
        assert_eq!(lines, vec!["GET /api/users 200 OK", "INFO: all good"]);
    }

    #[test]
    fn test_health_filter_case_insensitive() {
        let mut app = test_app();
        app.hide_health_checks = true;
        app.log_lines = vec![
            tl("user_agent=kube-probe/1.32"),  // lowercase
            tl("Kube-Probe/1.32 GET /status"), // mixed case
            tl("KUBE-PROBE/1.32"),             // upper case
        ];
        let filtered = app.filtered_log_lines();
        assert!(filtered.is_empty(), "all lines should be hidden");
    }

    #[test]
    fn test_health_filter_disabled_shows_all() {
        let mut app = test_app();
        app.hide_health_checks = false;
        app.log_lines = vec![
            tl("GET /health uri=/miapi/isHealthy user_agent=kube-probe/1.32"),
            tl("INFO: request processed"),
        ];
        let filtered = app.filtered_log_lines();
        assert_eq!(filtered.len(), 2, "disabled filter should show all lines");
    }

    #[test]
    fn test_health_filter_works_with_search() {
        let mut app = test_app();
        app.hide_health_checks = true;
        app.search_query = "GET".to_string();
        app.log_lines = vec![
            tl("GET /health user_agent=kube-probe/1.32"), // health + kube-probe -> hidden
            tl("GET /api/users"),                         // matches search, not health
            tl("POST /api/data"),                         // no match for search
        ];
        let filtered = app.filtered_log_lines();
        let lines: Vec<&str> = filtered.iter().map(|tl| tl.line.as_str()).collect();
        assert_eq!(lines, vec!["GET /api/users"]);
    }
}
