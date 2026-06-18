pub mod api;
pub mod auth;
pub mod config;
pub mod errors;
mod executor;
pub mod handler;
pub mod http;
pub mod ingest_router_service;
pub mod locality;
pub mod metrics_defs;
pub mod router;

#[cfg(test)]
mod testutils;

use crate::errors::IngestRouterError;
use auth::{RelaySigner, RelayVerifier};
use locator::client::Locator;
use shared::http::run_http_service;
use std::path::Path;

use shared::admin_service::AdminService;

pub async fn run(config: config::Config, credentials_path: &Path) -> Result<(), IngestRouterError> {
    let locator = Locator::new(config.locator.to_client_config()).await?;

    let verifier = RelayVerifier::from_relays(config.relay_keys)?;
    let signer = RelaySigner::from_file(credentials_path)?;

    let ingest_router_service = ingest_router_service::IngestRouterService::new(
        router::Router::new(config.routes, config.localities, locator.clone()),
        config.relay_timeouts,
        verifier,
        signer,
    );
    let admin_service = AdminService::new({
        let locator = locator.clone();
        move || locator.is_ready()
    });

    let router_task = run_http_service(
        &config.listener.host,
        config.listener.port,
        ingest_router_service,
    );
    let admin_task = run_http_service(
        &config.admin_listener.host,
        config.admin_listener.port,
        admin_service,
    );

    tokio::select! {
        result = async { tokio::try_join!(router_task, admin_task) } => {
            result?;
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutting down ingest-router...");
        }
    }

    locator.shutdown().await;

    Ok(())
}
