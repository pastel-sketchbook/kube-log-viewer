//! Integration tests that simulate mock K8s API responses as [`AppEvent`]s
//! and verify the full event pipeline: state transitions, error handling,
//! and multi-step flows (context switch → namespace load → pod load).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use kube_log_viewer::app::{App, Focus, InputMode, PopupKind};
use kube_log_viewer::event::AppEvent;
use kube_log_viewer::k8s::pods::PodInfo;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Create an [`App`] wired to a channel. Returns `(app, receiver)` so the
/// test can inspect events the app sends to background tasks.
fn test_app() -> (App, mpsc::UnboundedReceiver<AppEvent>) {
    let (tx, rx) = mpsc::unbounded_channel::<AppEvent>();
    (App::new(tx), rx)
}

/// Build a realistic [`PodInfo`] simulating a K8s API response.
fn mock_pod(
    name: &str,
    status: &str,
    containers: Vec<&str>,
    ready: usize,
    restarts: i32,
) -> PodInfo {
    let total = containers.len();
    PodInfo {
        name: name.to_string(),
        status: status.to_string(),
        ready: format!("{}/{}", ready, total),
        restarts,
        containers: containers.into_iter().map(String::from).collect(),
    }
}

// ---------------------------------------------------------------------------
// Full event pipeline: context load → namespace load → pod load
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_full_startup_flow() {
    let (mut app, _rx) = test_app();

    // 1. Simulate K8s returning contexts (mock kubeconfig read)
    app.handle_app_event(AppEvent::ContextsLoaded(
        vec!["aks-dev-westeu".to_string(), "aks-prod-westeu".to_string()],
        "aks-dev-westeu".to_string(),
    ));

    assert_eq!(app.contexts.len(), 2);
    assert_eq!(app.current_context, "aks-dev-westeu");

    // 2. Simulate K8s returning namespaces
    app.handle_app_event(AppEvent::NamespacesLoaded(
        "aks-dev-westeu".to_string(),
        vec![
            "default".to_string(),
            "ingress-nginx".to_string(),
            "kube-system".to_string(),
            "monitoring".to_string(),
        ],
    ));

    assert_eq!(app.namespaces.len(), 4);
    assert_eq!(app.current_namespace, "default");

    // 3. Simulate K8s returning pods in the default namespace
    app.handle_app_event(AppEvent::PodsUpdated(vec![
        mock_pod("nginx-7f8b6c9d4-abc12", "Running", vec!["nginx"], 1, 0),
        mock_pod(
            "app-backend-5d4f8c7b-xyz99",
            "Running",
            vec!["api", "sidecar-proxy"],
            2,
            3,
        ),
        mock_pod("job-migrate-db-lmn45", "Succeeded", vec!["migrate"], 0, 0),
    ]));

    assert_eq!(app.pods.len(), 3);
    // First pod auto-selected
    assert_eq!(app.pod_list_state.selected(), Some(0));
}

// ---------------------------------------------------------------------------
// Context switch resets state fully
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_context_switch_resets_state() {
    let (mut app, _rx) = test_app();

    // Seed initial state
    app.handle_app_event(AppEvent::ContextsLoaded(
        vec!["ctx-a".to_string(), "ctx-b".to_string()],
        "ctx-a".to_string(),
    ));
    app.handle_app_event(AppEvent::NamespacesLoaded(
        "ctx-a".to_string(),
        vec!["default".to_string(), "prod".to_string()],
    ));
    app.handle_app_event(AppEvent::PodsUpdated(vec![mock_pod(
        "web-1",
        "Running",
        vec!["web"],
        1,
        0,
    )]));

    // Select a pod and start receiving logs
    app.handle_key(key(KeyCode::Enter));
    app.handle_app_event(AppEvent::LogLine(
        "web-1".to_string(),
        "2024-01-15T10:00:00Z INFO started".to_string(),
    ));
    app.handle_app_event(AppEvent::LogLine(
        "web-1".to_string(),
        "2024-01-15T10:00:01Z INFO listening on :8080".to_string(),
    ));

    assert!(app.selected_pod.is_some());
    assert_eq!(app.log_lines.len(), 2);

    // Open context popup and switch to ctx-b
    app.handle_key(key(KeyCode::Char('c')));
    assert_eq!(app.popup, Some(PopupKind::Contexts));

    // Navigate to ctx-b (index 1) and confirm
    app.handle_key(key(KeyCode::Char('j')));
    app.handle_key(key(KeyCode::Enter));

    // State should be reset
    assert_eq!(app.current_context, "ctx-b");
    assert_eq!(app.current_namespace, "default");
    assert!(app.selected_pod.is_none());
    assert!(app.selected_container.is_none());
    assert!(app.log_lines.is_empty());
    assert!(app.pods.is_empty());
    assert!(app.namespaces.is_empty());
    assert!(app.containers.is_empty());
    assert!(app.popup.is_none());
}

// ---------------------------------------------------------------------------
// Namespace switch preserves context but resets pod/log state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_namespace_switch_resets_pods_and_logs() {
    let (mut app, _rx) = test_app();

    // Seed state
    app.handle_app_event(AppEvent::ContextsLoaded(
        vec!["my-ctx".to_string()],
        "my-ctx".to_string(),
    ));
    app.handle_app_event(AppEvent::NamespacesLoaded(
        "my-ctx".to_string(),
        vec!["default".to_string(), "staging".to_string()],
    ));
    app.handle_app_event(AppEvent::PodsUpdated(vec![mock_pod(
        "api-pod",
        "Running",
        vec!["api"],
        1,
        0,
    )]));

    // Select pod, receive logs
    app.handle_key(key(KeyCode::Enter));
    app.handle_app_event(AppEvent::LogLine(
        "api-pod".to_string(),
        "log line 1".to_string(),
    ));

    // Open namespace popup, switch to staging
    app.handle_key(key(KeyCode::Char('n')));
    assert_eq!(app.popup, Some(PopupKind::Namespaces));
    app.handle_key(key(KeyCode::Char('j'))); // move to "staging"
    app.handle_key(key(KeyCode::Enter));

    // Context preserved, namespace changed, pod/log state reset
    assert_eq!(app.current_context, "my-ctx");
    assert_eq!(app.current_namespace, "staging");
    assert!(app.selected_pod.is_none());
    assert!(app.log_lines.is_empty());
    assert!(app.popup.is_none());

    // Simulate new pods arriving from the staging namespace
    app.handle_app_event(AppEvent::PodsUpdated(vec![
        mock_pod("staging-worker-1", "Running", vec!["worker"], 1, 0),
        mock_pod("staging-worker-2", "Pending", vec!["worker"], 0, 0),
    ]));

    assert_eq!(app.pods.len(), 2);
    assert_eq!(app.pods[1].status, "Pending");
}

// ---------------------------------------------------------------------------
// Container switch within a multi-container pod
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_container_switch_clears_logs() {
    let (mut app, _rx) = test_app();

    // Seed with a multi-container pod
    app.handle_app_event(AppEvent::ContextsLoaded(
        vec!["ctx".to_string()],
        "ctx".to_string(),
    ));
    app.handle_app_event(AppEvent::NamespacesLoaded(
        "ctx".to_string(),
        vec!["default".to_string()],
    ));
    app.handle_app_event(AppEvent::PodsUpdated(vec![mock_pod(
        "multi-pod",
        "Running",
        vec!["app", "istio-proxy", "fluentd"],
        3,
        0,
    )]));

    // Select the pod (auto-selects first container "app")
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.selected_container, Some("app".to_string()));
    assert_eq!(app.containers.len(), 3);

    // Receive some logs from the "app" container
    app.handle_app_event(AppEvent::LogLine(
        "multi-pod".to_string(),
        "app: request served".to_string(),
    ));
    app.handle_app_event(AppEvent::LogLine(
        "multi-pod".to_string(),
        "app: 200 OK".to_string(),
    ));
    assert_eq!(app.log_lines.len(), 2);

    // Open container popup, switch to istio-proxy (index 1)
    app.handle_key(key(KeyCode::Char('s')));
    assert_eq!(app.popup, Some(PopupKind::Containers));
    app.handle_key(key(KeyCode::Char('j'))); // move to "istio-proxy"
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.selected_container, Some("istio-proxy".to_string()));
    // Logs cleared for the new container stream
    assert!(app.log_lines.is_empty());
    assert!(app.follow_mode);
    assert_eq!(app.log_scroll_offset, 0);
}

// ---------------------------------------------------------------------------
// Error handling: error displayed, then cleared on success
// ---------------------------------------------------------------------------

#[test]
fn test_error_then_recovery() {
    let (mut app, _rx) = test_app();

    // Simulate a connection error (e.g. bad kubeconfig, cluster unreachable)
    app.handle_app_event(AppEvent::Error(
        "Failed to load namespaces: failed to load kubeconfig for context 'bad-ctx'".to_string(),
    ));

    // Error appended to log lines as [ERROR] prefix
    assert_eq!(app.log_lines.len(), 1);
    assert!(app.log_lines[0].line.starts_with("[ERROR]"));
    assert!(app.log_lines[0].line.contains("kubeconfig"));

    // Simulate recovery -- namespaces load successfully
    app.handle_app_event(AppEvent::NamespacesLoaded(
        String::new(),
        vec!["default".to_string()],
    ));
    assert_eq!(app.namespaces, vec!["default"]);
}

#[test]
fn test_multiple_errors_accumulate_in_log_lines() {
    let (mut app, _rx) = test_app();

    app.handle_app_event(AppEvent::Error("error 1".to_string()));
    app.handle_app_event(AppEvent::Error("error 2".to_string()));

    assert_eq!(app.log_lines.len(), 2);
    assert_eq!(app.log_lines[0].line, "[ERROR] error 1");
    assert_eq!(app.log_lines[1].line, "[ERROR] error 2");
}

// ---------------------------------------------------------------------------
// Log streaming with follow mode and search filtering
// ---------------------------------------------------------------------------

#[test]
fn test_log_stream_with_search_filter() {
    let (mut app, _rx) = test_app();

    // Receive a batch of log lines simulating a real pod's output
    let log_lines = vec![
        "2024-01-15T10:00:00Z INFO  server starting on port 8080",
        "2024-01-15T10:00:01Z DEBUG loading configuration from /etc/config",
        "2024-01-15T10:00:02Z INFO  connected to database at postgres:5432",
        "2024-01-15T10:00:03Z WARN  connection pool near capacity (45/50)",
        "2024-01-15T10:00:04Z ERROR failed to process request: timeout after 30s",
        "2024-01-15T10:00:05Z INFO  request completed in 150ms",
        "2024-01-15T10:00:06Z ERROR database connection lost, retrying...",
        "2024-01-15T10:00:07Z INFO  database reconnected successfully",
    ];

    for line in &log_lines {
        app.handle_app_event(AppEvent::LogLine(String::new(), line.to_string()));
    }

    assert_eq!(app.log_lines.len(), 8);

    // Search for errors
    app.handle_key(key(KeyCode::Char('/')));
    assert_eq!(app.input_mode, InputMode::Search);
    app.handle_key(key(KeyCode::Char('E')));
    app.handle_key(key(KeyCode::Char('R')));
    app.handle_key(key(KeyCode::Char('R')));
    app.handle_key(key(KeyCode::Char('O')));
    app.handle_key(key(KeyCode::Char('R')));
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.input_mode, InputMode::Normal);
    assert_eq!(app.search_query, "ERROR");

    let filtered = app.filtered_log_lines();
    assert_eq!(filtered.len(), 2);
    assert!(filtered[0].line.contains("timeout after 30s"));
    assert!(filtered[1].line.contains("database connection lost"));

    // Clear search with Esc
    app.handle_key(key(KeyCode::Esc));
    assert!(app.search_query.is_empty());
    assert_eq!(app.filtered_log_lines().len(), 8);
}

// ---------------------------------------------------------------------------
// Log follow mode and scroll interaction
// ---------------------------------------------------------------------------

#[test]
fn test_follow_mode_tracks_new_lines() {
    let (mut app, _rx) = test_app();
    app.focus = Focus::Logs;
    assert!(app.follow_mode);

    // Receive lines -- offset should track latest
    for i in 0..50 {
        app.handle_app_event(AppEvent::LogLine(String::new(), format!("line {i}")));
    }

    let offset_after_follow = app.log_scroll_offset;
    assert!(offset_after_follow > 0);

    // Scroll up disables follow
    app.handle_key(key(KeyCode::Char('k')));
    assert!(!app.follow_mode);
    let offset_after_scroll = app.log_scroll_offset;
    assert!(offset_after_scroll < offset_after_follow);

    // New lines arrive but offset stays (not following)
    app.handle_app_event(AppEvent::LogLine(
        String::new(),
        "new line while scrolled up".to_string(),
    ));
    assert_eq!(app.log_scroll_offset, offset_after_scroll);

    // G re-enables follow
    app.handle_key(key(KeyCode::Char('G')));
    assert!(app.follow_mode);
}

// ---------------------------------------------------------------------------
// Namespace fallback when current namespace disappears
// ---------------------------------------------------------------------------

#[test]
fn test_namespace_fallback_after_context_switch() {
    let (mut app, _rx) = test_app();

    app.current_namespace = "my-custom-ns".to_string();

    // New context has different namespaces -- current_namespace not present
    app.handle_app_event(AppEvent::NamespacesLoaded(
        String::new(),
        vec!["default".to_string(), "kube-system".to_string()],
    ));

    // Falls back to first namespace
    assert_eq!(app.current_namespace, "default");
}

#[test]
fn test_namespace_preserved_when_still_present() {
    let (mut app, _rx) = test_app();

    app.current_namespace = "monitoring".to_string();

    app.handle_app_event(AppEvent::NamespacesLoaded(
        String::new(),
        vec![
            "default".to_string(),
            "monitoring".to_string(),
            "kube-system".to_string(),
        ],
    ));

    // Preserved because "monitoring" is in the list
    assert_eq!(app.current_namespace, "monitoring");
}

// ---------------------------------------------------------------------------
// Pod selection does not crash on empty list
// ---------------------------------------------------------------------------

#[test]
fn test_enter_on_empty_pod_list_is_safe() {
    let (mut app, _rx) = test_app();
    app.focus = Focus::Pods;
    app.pods.clear();
    app.pod_list_state.select(None);

    // Should be a no-op, not a panic
    app.handle_key(key(KeyCode::Enter));
    assert!(app.selected_pod.is_none());
}

// ---------------------------------------------------------------------------
// Log line cap under sustained load
// ---------------------------------------------------------------------------

#[test]
fn test_log_line_cap_under_sustained_load() {
    let (mut app, _rx) = test_app();

    // Simulate a very chatty pod: 60k lines
    for i in 0..60_000 {
        app.handle_app_event(AppEvent::LogLine(
            String::new(),
            format!("2024-01-15T10:00:00Z INFO request {i} processed"),
        ));
    }

    // Cap is 50,000; after exceeding, drains 10,000
    // So we should be between 40,000 and 50,001
    assert!(app.log_lines.len() <= 50_001);
    assert!(app.log_lines.len() >= 40_000);
}
