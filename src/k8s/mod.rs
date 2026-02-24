pub mod contexts;
pub mod logs;
pub mod namespaces;
pub mod pods;

use anyhow::{Context, Result};
use kube::config::KubeConfigOptions;
use kube::{Client, Config};

/// Create a [`kube::Client`] for the given context name.
///
/// If `context` is `None`, the default kubeconfig context is used.
pub async fn create_client(context: Option<&str>) -> Result<Client> {
    let config = match context {
        Some(ctx) => {
            let options = KubeConfigOptions {
                context: Some(ctx.to_string()),
                ..Default::default()
            };
            Config::from_kubeconfig(&options)
                .await
                .with_context(|| format!("failed to load kubeconfig for context '{ctx}'"))?
        }
        None => Config::infer()
            .await
            .context("failed to infer kubeconfig -- is ~/.kube/config present?")?,
    };
    Client::try_from(config).context("failed to create kubernetes client")
}
