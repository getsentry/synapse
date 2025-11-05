mod admin_service;
pub mod config;
mod errors;
mod locator;
mod proxy_service;
mod resolvers;
mod route_actions;
mod service;
mod upstreams;

use crate::errors::ProxyError;
use crate::locator::Locator;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use hyper_util::server::conn::auto::Builder;
use service::ServiceType;
use shared::metrics::Metrics;
use std::sync::Arc;
use tokio::net::TcpListener;

pub async fn run(config: config::Config, metrics: Metrics) -> Result<(), ProxyError> {
    let locator = Locator::new_from_config(config.locator.clone(), metrics);

    let proxy_task = run_task(
        &config.listener.host,
        config.listener.port,
        ServiceType::Proxy(Box::new(proxy_service::ProxyService::try_new(
            locator.clone(),
            config.routes,
            config.upstreams,
        )?)),
    );
    let admin_task = run_task(
        &config.admin_listener.host,
        config.admin_listener.port,
        ServiceType::Admin(Box::new(admin_service::AdminService::new(locator))),
    );

    tokio::try_join!(proxy_task, admin_task)?;

    Ok(())
}

async fn run_task(host: &str, port: u16, service: ServiceType) -> Result<(), ProxyError> {
    let listener = TcpListener::bind(format!("{host}:{port}")).await?;

    let service_arc = Arc::new(service);

    loop {
        let (stream, _peer_addr) = listener.accept().await?;
        let _ = stream.set_nodelay(true);
        let io = TokioIo::new(stream);
        let svc = service_arc.clone();

        // Hand the connection to hyper; auto-detect h1/h2 on this socket
        tokio::spawn(async move {
            let _ = Builder::new(TokioExecutor::new())
                .serve_connection(io, svc)
                .await;
        });
    }
}
