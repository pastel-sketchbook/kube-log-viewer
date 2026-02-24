use crate::k8s::pods::PodInfo;

#[derive(Debug)]
pub enum AppEvent {
    /// Pod list fetched from K8s
    PodsUpdated(Vec<PodInfo>),
    /// Namespace list fetched from K8s
    NamespacesLoaded(Vec<String>),
    /// Context list + current context loaded from kubeconfig
    ContextsLoaded(Vec<String>, String),
    /// A single log line received from a pod's log stream
    LogLine(String),
    /// The log stream has ended (pod terminated, stream closed, etc.)
    LogStreamEnded,
    /// An error from a background K8s operation
    Error(String),
}
