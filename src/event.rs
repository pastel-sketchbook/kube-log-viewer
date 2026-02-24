use crate::k8s::pods::PodInfo;

#[derive(Debug)]
pub enum AppEvent {
    /// Pod list fetched from K8s
    PodsUpdated(Vec<PodInfo>),
    /// Namespace list fetched from K8s.
    /// Fields: (context the namespaces belong to, namespace names).
    NamespacesLoaded(String, Vec<String>),
    /// Context list + current context loaded from kubeconfig
    ContextsLoaded(Vec<String>, String),
    /// A single log line received from a pod's log stream.
    /// Fields: (source pod name, log line text).
    /// Empty source string indicates a system/internal message.
    LogLine(String, String),
    /// The log stream has ended (pod terminated, stream closed, etc.)
    LogStreamEnded,
    /// `az login` completed (success or failure message)
    AzLoginCompleted(Result<(), String>),
    /// Log export completed successfully — payload is the file path
    ExportCompleted(String),
    /// An error from a background K8s operation
    Error(String),
}
