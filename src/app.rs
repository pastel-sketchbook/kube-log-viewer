use std::time::Duration;

use anyhow::{Context as _, Result};
use chrono::Utc;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use ratatui::prelude::*;
use ratatui::widgets::ListState;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::event::AppEvent;
use crate::k8s;
use crate::ui;
use crate::ui::theme::{THEMES, Theme};

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
}

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
    pub input_mode: InputMode,
    pub search_query: String,
    pub show_help: bool,

    // Popup state
    pub popup: Option<PopupKind>,
    pub popup_list_state: ListState,

    // Log state
    pub log_lines: Vec<String>,
    pub selected_pod: Option<String>,
    pub selected_container: Option<String>,
    pub containers: Vec<String>,

    // Theme
    pub theme_index: usize,

    // Auth state
    pub az_login_in_progress: bool,

    // Control
    pub should_quit: bool,

    // Channel for sending events from background tasks
    tx: mpsc::UnboundedSender<AppEvent>,

    // Log stream cancellation handle
    log_cancel_tx: Option<tokio::sync::watch::Sender<bool>>,
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
            input_mode: InputMode::Normal,
            search_query: String::new(),
            show_help: false,

            popup: None,
            popup_list_state: ListState::default(),

            log_lines: Vec::new(),
            selected_pod: None,
            selected_container: None,
            containers: Vec::new(),

            theme_index: 0,

            az_login_in_progress: false,

            should_quit: false,
            tx,
            log_cancel_tx: None,
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
                        Some(Ok(Event::Resize(_, _))) => { /* re-render on next loop */ }
                        Some(Err(_)) | None => break,
                        _ => {}
                    }
                }
                maybe_event = rx.recv() => {
                    match maybe_event {
                        Some(event) => app.handle_app_event(event),
                        None => break,
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

        // Clean up any running log stream
        app.cancel_log_stream();
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
            KeyCode::Char('f') => self.follow_mode = !self.follow_mode,
            KeyCode::Char('w') => self.wide_logs = !self.wide_logs,
            KeyCode::Char('W') => self.wrap_lines = !self.wrap_lines,
            KeyCode::Char('J') => self.json_mode = !self.json_mode,
            KeyCode::Char('T') => self.timestamp_mode = self.timestamp_mode.cycle(),
            KeyCode::Char('R') => self.open_popup(PopupKind::TimeRange),
            KeyCode::Char('t') => self.cycle_theme(),
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
        match self.focus {
            Focus::Pods => {
                let i = self.pod_list_state.selected().unwrap_or(0);
                if i > 0 {
                    self.pod_list_state.select(Some(i - 1));
                }
            }
            Focus::Logs => {
                self.follow_mode = false;
                self.log_scroll_offset = self.log_scroll_offset.saturating_sub(1);
            }
        }
    }

    fn navigate_down(&mut self) {
        match self.focus {
            Focus::Pods => {
                let len = self.pods.len();
                let i = self.pod_list_state.selected().unwrap_or(0);
                if len > 0 && i + 1 < len {
                    self.pod_list_state.select(Some(i + 1));
                }
            }
            Focus::Logs => {
                self.follow_mode = false;
                let max = self.filtered_log_lines().len().saturating_sub(1);
                if self.log_scroll_offset < max {
                    self.log_scroll_offset += 1;
                }
            }
        }
    }

    fn scroll_to_bottom(&mut self) {
        if self.focus == Focus::Logs {
            self.follow_mode = true;
            self.log_scroll_offset = self.filtered_log_lines().len().saturating_sub(1);
        }
    }

    fn scroll_to_top(&mut self) {
        if self.focus == Focus::Logs {
            self.follow_mode = false;
            self.log_scroll_offset = 0;
        }
    }

    fn page_up(&mut self) {
        if self.focus == Focus::Logs {
            self.follow_mode = false;
            self.log_scroll_offset = self.log_scroll_offset.saturating_sub(20);
        }
    }

    fn page_down(&mut self) {
        if self.focus == Focus::Logs {
            let max = self.filtered_log_lines().len().saturating_sub(1);
            self.log_scroll_offset = (self.log_scroll_offset + 20).min(max);
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
                    self.cancel_log_stream();
                    self.load_pods();
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
                    self.cancel_log_stream();
                    self.load_namespaces();
                    self.load_pods();
                }
            }
            PopupKind::Containers => {
                if let Some(container) = self.containers.get(i).cloned() {
                    info!(container = %container, "switching container");
                    self.selected_container = Some(container);
                    self.log_lines.clear();
                    self.log_scroll_offset = 0;
                    self.follow_mode = true;

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
                self.load_pods();
            }
            AppEvent::NamespacesLoaded(namespaces) => {
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
            AppEvent::PodsUpdated(pods) => {
                debug!(count = pods.len(), "pods updated");
                self.pods = pods;
                if self.pod_list_state.selected().is_none() && !self.pods.is_empty() {
                    self.pod_list_state.select(Some(0));
                }
            }
            AppEvent::LogLine(line) => {
                self.log_lines.push(line);
                if self.follow_mode {
                    self.log_scroll_offset = self.filtered_log_lines().len().saturating_sub(1);
                }
                // Cap log lines to prevent unbounded memory growth
                if self.log_lines.len() > 50_000 {
                    debug!("log line cap reached, draining oldest 10,000 lines");
                    self.log_lines.drain(..10_000);
                }
            }
            AppEvent::LogStreamEnded => {
                debug!("log stream ended");
            }
            AppEvent::AzLoginCompleted(result) => {
                self.az_login_in_progress = false;
                match result {
                    Ok(()) => {
                        self.log_lines.push(
                            "[INFO] az login succeeded — reloading cluster data…".to_string(),
                        );
                        self.load_contexts();
                    }
                    Err(msg) => {
                        error!(message = %msg, "az login failed");
                        self.log_lines
                            .push(format!("[ERROR] az login failed: {msg}"));
                    }
                }
            }
            AppEvent::Error(msg) => {
                error!(message = %msg, "background task error");
                if is_auth_error(&msg) {
                    self.log_lines.push(format!(
                        "[ERROR] {msg} (Azure credentials expired — run `az login`)"
                    ));
                    if !self.az_login_in_progress {
                        self.spawn_az_login();
                    }
                } else {
                    self.log_lines.push(format!("[ERROR] {msg}"));
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
    }

    pub fn filtered_log_lines(&self) -> Vec<&str> {
        let search_lower = if self.search_query.is_empty() {
            None
        } else {
            Some(self.search_query.to_lowercase())
        };

        let cutoff = match self.time_range {
            TimeRange::All => None,
            TimeRange::Last(dur) => {
                let now = Utc::now();
                Some(now - chrono::Duration::from_std(dur).unwrap_or(chrono::Duration::zero()))
            }
        };

        self.log_lines
            .iter()
            .filter(|line| {
                // Search filter
                if let Some(ref lower) = search_lower
                    && !line.to_lowercase().contains(lower)
                {
                    return false;
                }
                // Time range filter
                if let Some(cutoff_dt) = cutoff
                    && let Some(m) = ui::logs::TIMESTAMP_RE.find(line.as_str())
                    && let Some(dt) = ui::logs::parse_log_timestamp(&line[..m.end()])
                {
                    return dt >= cutoff_dt;
                }
                // Lines without parseable timestamps are always included
                true
            })
            .map(|s| s.as_str())
            .collect()
    }

    // -- K8s background operations ------------------------------------------

    fn load_contexts(&self) {
        info!("spawning context load task");
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match k8s::contexts::load_contexts() {
                Ok((contexts, current)) => {
                    let _ = tx.send(AppEvent::ContextsLoaded(contexts, current));
                }
                Err(e) => {
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
                    let _ = tx.send(AppEvent::NamespacesLoaded(namespaces));
                }
                Err(e) => {
                    error!(error = %e, "failed to load namespaces");
                    let _ = tx.send(AppEvent::Error(format!("Failed to load namespaces: {e:#}")));
                }
            }
        });
    }

    fn load_pods(&self) {
        info!(
            context = %self.current_context,
            namespace = %self.current_namespace,
            "spawning pod load task"
        );
        let tx = self.tx.clone();
        let context = self.current_context.clone();
        let namespace = self.current_namespace.clone();
        tokio::spawn(async move {
            match k8s::pods::list_pods(&context, &namespace).await {
                Ok(pods) => {
                    let _ = tx.send(AppEvent::PodsUpdated(pods));
                }
                Err(e) => {
                    error!(error = %e, "failed to load pods");
                    let _ = tx.send(AppEvent::Error(format!("Failed to load pods: {e:#}")));
                }
            }
        });
    }

    fn start_log_stream(&mut self, pod_name: &str, container: Option<&str>) {
        info!(pod = pod_name, container = container, "starting log stream");
        self.cancel_log_stream();

        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        self.log_cancel_tx = Some(cancel_tx);

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
                tx.clone(),
            )
            .await
            {
                Ok(()) => {
                    let _ = tx.send(AppEvent::LogStreamEnded);
                }
                Err(e) => {
                    error!(error = %e, "log stream failed");
                    let _ = tx.send(AppEvent::Error(format!("Log stream error: {e:#}")));
                }
            }
        });
    }

    fn cancel_log_stream(&mut self) {
        if let Some(cancel_tx) = self.log_cancel_tx.take() {
            debug!("cancelling active log stream");
            let _ = cancel_tx.send(true);
        }
    }

    /// Spawn `az login` in the background and send [`AppEvent::AzLoginCompleted`]
    /// when it finishes. Opens the default browser for interactive auth.
    fn spawn_az_login(&mut self) {
        self.az_login_in_progress = true;
        self.log_lines
            .push("[INFO] Azure credentials expired — opening browser for login…".to_string());
        info!("spawning az login");

        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = tokio::process::Command::new("az")
                .arg("login")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn();

            let child = match result {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(AppEvent::AzLoginCompleted(Err(format!(
                        "spawn failed: {e}"
                    ))));
                    return;
                }
            };

            match child.wait_with_output().await {
                Ok(output) if output.status.success() => {
                    let _ = tx.send(AppEvent::AzLoginCompleted(Ok(())));
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let msg = stderr.lines().last().unwrap_or("unknown error");
                    let _ = tx.send(AppEvent::AzLoginCompleted(Err(msg.to_string())));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::AzLoginCompleted(Err(format!("wait failed: {e}"))));
                }
            }
        });
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
        app.log_lines = vec!["a".to_string(), "b".to_string(), "c".to_string()];
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
        app.log_lines = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
        let filtered = app.filtered_log_lines();
        assert_eq!(filtered, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn test_filtered_log_lines_with_query() {
        let mut app = test_app();
        app.log_lines = vec![
            "INFO: started".to_string(),
            "ERROR: failed".to_string(),
            "INFO: completed".to_string(),
        ];
        app.search_query = "error".to_string();
        let filtered = app.filtered_log_lines();
        assert_eq!(filtered, vec!["ERROR: failed"]);
    }

    #[test]
    fn test_filtered_log_lines_case_insensitive() {
        let mut app = test_app();
        app.log_lines = vec!["Error occurred".to_string(), "all good".to_string()];
        app.search_query = "ERROR".to_string();
        let filtered = app.filtered_log_lines();
        assert_eq!(filtered, vec!["Error occurred"]);
    }

    #[test]
    fn test_filtered_log_lines_no_match() {
        let mut app = test_app();
        app.log_lines = vec!["hello world".to_string()];
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
        app.handle_app_event(AppEvent::NamespacesLoaded(vec![
            "default".to_string(),
            "kube-system".to_string(),
        ]));
        assert_eq!(app.current_namespace, "kube-system");
    }

    #[test]
    fn test_namespaces_loaded_falls_back_to_first() {
        let mut app = test_app();
        app.current_namespace = "nonexistent".to_string();
        app.handle_app_event(AppEvent::NamespacesLoaded(vec![
            "default".to_string(),
            "production".to_string(),
        ]));
        assert_eq!(app.current_namespace, "default");
    }

    #[test]
    fn test_log_line_appended() {
        let mut app = test_app();
        app.handle_app_event(AppEvent::LogLine("hello".to_string()));
        assert_eq!(app.log_lines, vec!["hello"]);
    }

    #[test]
    fn test_error_shown_in_log_lines() {
        let mut app = test_app();
        app.handle_app_event(AppEvent::Error("connection refused".to_string()));
        assert_eq!(app.log_lines, vec!["[ERROR] connection refused"]);
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
                .is_some_and(|l| l.contains("succeeded"))
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
                .is_some_and(|l| l.contains("az login failed"))
        );
    }

    #[test]
    fn test_log_line_cap() {
        let mut app = test_app();
        for i in 0..50_001 {
            app.log_lines.push(format!("line {i}"));
        }
        app.handle_app_event(AppEvent::LogLine("overflow".to_string()));
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
        app.log_lines = vec![
            "2020-01-01T00:00:00Z old line".to_string(),
            "no timestamp here".to_string(),
        ];
        let filtered = app.filtered_log_lines();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filtered_log_lines_time_range_excludes_old() {
        let mut app = test_app();
        // Set range to last 5 minutes — a timestamp from 2020 should be excluded
        app.time_range = TimeRange::Last(Duration::from_secs(5 * 60));
        app.log_lines = vec!["2020-01-01T00:00:00Z ancient log entry".to_string()];
        let filtered = app.filtered_log_lines();
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_filtered_log_lines_time_range_includes_recent() {
        let mut app = test_app();
        app.time_range = TimeRange::Last(Duration::from_secs(60 * 60));
        // Generate a timestamp that is "now" so it falls within the 1h range
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        app.log_lines = vec![format!("{now} recent log entry")];
        let filtered = app.filtered_log_lines();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filtered_log_lines_time_range_includes_unparseable() {
        let mut app = test_app();
        app.time_range = TimeRange::Last(Duration::from_secs(5 * 60));
        app.log_lines = vec![
            "no timestamp at all".to_string(),
            "just some text".to_string(),
        ];
        // Lines without parseable timestamps are always included
        let filtered = app.filtered_log_lines();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filtered_log_lines_time_range_with_search() {
        let mut app = test_app();
        app.time_range = TimeRange::Last(Duration::from_secs(5 * 60));
        app.search_query = "error".to_string();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        app.log_lines = vec![
            format!("{now} INFO: all good"),         // recent but no match
            format!("{now} ERROR: something broke"), // recent and matches
            "2020-01-01T00:00:00Z ERROR: ancient".to_string(), // matches but too old
            "no timestamp ERROR here".to_string(),   // matches, no timestamp (included)
        ];
        let filtered = app.filtered_log_lines();
        assert_eq!(filtered.len(), 2);
        assert!(filtered[0].contains("ERROR: something broke"));
        assert!(filtered[1].contains("no timestamp ERROR here"));
    }
}
