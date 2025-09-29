mod admin_service;
pub mod config;
mod proxy_service;
mod route_actions;
mod service;

use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use hyper_util::server::conn::auto::Builder;
use service::ServiceType;
use std::io;
use std::process;
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(thiserror::Error, Debug)]
pub enum ProxyError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

pub async fn run(config: config::Config) {
    let proxy_task = run_task(
        &config.listener.host,
        config.listener.port,
        ServiceType::Proxy(Box::new(proxy_service::ProxyService::new(config.clone()))),
    );
    let admin_task = run_task(
        &config.admin_listener.host,
        config.admin_listener.port,
        ServiceType::Admin(Box::new(admin_service::AdminService::new())),
    );

    if let Err(e) = tokio::try_join!(proxy_task, admin_task) {
        eprintln!("Admin error: {}", e);
        process::exit(1);
    }
}

async fn run_task(host: &str, port: u16, service: ServiceType) -> Result<(), ProxyError> {
    let listener = TcpListener::bind(format!("{}:{}", host, port)).await?;

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
