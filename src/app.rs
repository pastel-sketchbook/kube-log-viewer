use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use ratatui::prelude::*;
use ratatui::widgets::ListState;
use tokio::sync::mpsc;

use crate::event::AppEvent;
use crate::k8s;
use crate::ui;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
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
            input_mode: InputMode::Normal,
            search_query: String::new(),
            show_help: false,

            popup: None,
            popup_list_state: ListState::default(),

            log_lines: Vec::new(),
            selected_pod: None,
            selected_container: None,
            containers: Vec::new(),

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
            terminal.draw(|f| ui::render(f, &mut app))?;

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

    fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl+C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        if self.popup.is_some() {
            self.handle_popup_key(key);
            return;
        }

        if self.input_mode == InputMode::Search {
            self.handle_search_key(key);
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Char('n') => self.open_popup(PopupKind::Namespaces),
            KeyCode::Char('c') => self.open_popup(PopupKind::Contexts),
            KeyCode::Char('s') => {
                if !self.containers.is_empty() {
                    self.open_popup(PopupKind::Containers);
                }
            }
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Search;
                self.search_query.clear();
            }
            KeyCode::Char('f') => self.follow_mode = !self.follow_mode,
            KeyCode::Char('w') => self.wrap_lines = !self.wrap_lines,
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
            KeyCode::Esc => self.input_mode = InputMode::Normal,
            KeyCode::Enter => self.input_mode = InputMode::Normal,
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
        };

        self.popup_list_state.select(selected.or(Some(0)));
        self.popup = Some(kind);
    }

    fn popup_items_len(&self) -> usize {
        match self.popup {
            Some(PopupKind::Namespaces) => self.namespaces.len(),
            Some(PopupKind::Contexts) => self.contexts.len(),
            Some(PopupKind::Containers) => self.containers.len(),
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
                    self.current_context = ctx;
                    self.current_namespace = String::from("default");
                    self.selected_pod = None;
                    self.selected_container = None;
                    self.log_lines.clear();
                    self.pods.clear();
                    self.namespaces.clear();
                    self.containers.clear();
                    self.cancel_log_stream();
                    self.switch_context();
                }
            }
            PopupKind::Containers => {
                if let Some(container) = self.containers.get(i).cloned() {
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
        }

        self.popup = None;
    }

    // -- App events from background tasks -----------------------------------

    fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::ContextsLoaded(contexts, current) => {
                self.contexts = contexts;
                self.current_context = current;
                self.load_namespaces();
                self.load_pods();
            }
            AppEvent::NamespacesLoaded(namespaces) => {
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
                    self.log_lines.drain(..10_000);
                }
            }
            AppEvent::LogStreamEnded => {
                // Stream ended naturally; no action needed
            }
            AppEvent::Error(msg) => {
                self.log_lines.push(format!("[ERROR] {}", msg));
            }
        }
    }

    // -- Helpers ------------------------------------------------------------

    pub fn filtered_log_lines(&self) -> Vec<&str> {
        if self.search_query.is_empty() {
            self.log_lines.iter().map(|s| s.as_str()).collect()
        } else {
            let query = self.search_query.to_lowercase();
            self.log_lines
                .iter()
                .filter(|line| line.to_lowercase().contains(&query))
                .map(|s| s.as_str())
                .collect()
        }
    }

    // -- K8s background operations ------------------------------------------

    fn load_contexts(&self) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match k8s::contexts::load_contexts() {
                Ok((contexts, current)) => {
                    let _ = tx.send(AppEvent::ContextsLoaded(contexts, current));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::Error(format!("Failed to load contexts: {e}")));
                }
            }
        });
    }

    fn load_namespaces(&self) {
        let tx = self.tx.clone();
        let context = self.current_context.clone();
        tokio::spawn(async move {
            match k8s::namespaces::list_namespaces(&context).await {
                Ok(namespaces) => {
                    let _ = tx.send(AppEvent::NamespacesLoaded(namespaces));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::Error(format!("Failed to load namespaces: {e}")));
                }
            }
        });
    }

    fn load_pods(&self) {
        let tx = self.tx.clone();
        let context = self.current_context.clone();
        let namespace = self.current_namespace.clone();
        tokio::spawn(async move {
            match k8s::pods::list_pods(&context, &namespace).await {
                Ok(pods) => {
                    let _ = tx.send(AppEvent::PodsUpdated(pods));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::Error(format!("Failed to load pods: {e}")));
                }
            }
        });
    }

    fn start_log_stream(&mut self, pod_name: &str, container: Option<&str>) {
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
                    let _ = tx.send(AppEvent::Error(format!("Log stream error: {e}")));
                }
            }
        });
    }

    fn cancel_log_stream(&mut self) {
        if let Some(cancel_tx) = self.log_cancel_tx.take() {
            let _ = cancel_tx.send(true);
        }
    }

    fn switch_context(&self) {
        let tx = self.tx.clone();
        let context = self.current_context.clone();
        tokio::spawn(async move {
            match k8s::namespaces::list_namespaces(&context).await {
                Ok(namespaces) => {
                    let _ = tx.send(AppEvent::NamespacesLoaded(namespaces));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::Error(format!("Failed to load namespaces: {e}")));
                }
            }
            match k8s::pods::list_pods(&context, "default").await {
                Ok(pods) => {
                    let _ = tx.send(AppEvent::PodsUpdated(pods));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::Error(format!("Failed to load pods: {e}")));
                }
            }
        });
    }
}
