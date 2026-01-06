pub mod api;
pub mod config;
pub mod errors;
mod executor;
pub mod handler;
pub mod http;
pub mod ingest_router_service;
pub mod locale;
pub mod router;

#[cfg(test)]
mod testutils;

use crate::errors::IngestRouterError;
use locator::client::Locator;
use shared::http::run_http_service;

pub async fn run(config: config::Config) -> Result<(), IngestRouterError> {
    let locator = Locator::new(config.locator.to_client_config()).await?;

    let ingest_router_service = ingest_router_service::IngestRouterService::new(
        router::Router::new(config.routes, config.locales, locator),
        config.relay_timeouts,
    );

    let router_task = run_http_service(
        &config.listener.host,
        config.listener.port,
        ingest_router_service,
    );
    router_task.await?;
    Ok(())
}
