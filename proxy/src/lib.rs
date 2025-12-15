mod admin_service;
pub mod config;
mod errors;
mod proxy_service;
mod resolvers;
mod route_actions;
mod upstreams;

use crate::errors::ProxyError;
use locator::client::Locator;
use shared::http::run_http_service;

pub async fn run(config: config::Config) -> Result<(), ProxyError> {
    let locator = Locator::new(config.locator.to_client_config()).await?;

    let proxy_service =
        proxy_service::ProxyService::try_new(locator.clone(), config.routes, config.upstreams)?;
    let admin_service = admin_service::AdminService::new(locator.clone());

    let proxy_task = run_http_service(&config.listener.host, config.listener.port, proxy_service);
    let admin_task = run_http_service(
        &config.admin_listener.host,
        config.admin_listener.port,
        admin_service,
    );

    tokio::select! {
        result = async { tokio::try_join!(proxy_task, admin_task) } => {
            result?;
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutting down proxy...");
        }
    }

    locator.shutdown().await;

    Ok(())
}
