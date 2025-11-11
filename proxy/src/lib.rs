mod admin_service;
pub mod config;
mod errors;
mod locator;
mod proxy_service;
mod resolvers;
mod route_actions;
mod upstreams;
mod utils;

use crate::errors::ProxyError;
use crate::locator::Locator;
use shared::http::run_http_service;

pub async fn run(config: config::Config) -> Result<(), ProxyError> {
    let locator = Locator::new_from_config(config.locator.clone());

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
