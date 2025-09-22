pub mod config;
mod route_actions;
mod service;

use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use hyper_util::server::conn::auto::Builder;
use std::io;
use std::process;
use std::sync::Arc;
use tokio::net::TcpListener;

pub fn run(proxy_config: config::Config) {
    println!("Starting proxy server on 127.0.0.1:3000");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    match rt.block_on(run_async(proxy_config)) {
        Ok(_) => println!("Proxy server exited"),
        Err(e) => {
            println!("Proxy server exited with error {:?}", e);
            process::exit(1);
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ProxyError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

async fn run_async(proxy_config: config::Config) -> Result<(), ProxyError> {
    let listener = TcpListener::bind("127.0.0.1:3000").await?;

    let proxy_service = Arc::new(service::ProxyService::new(proxy_config));

    loop {
        let (stream, _peer_addr) = listener.accept().await?;
        let _ = stream.set_nodelay(true);
        let io = TokioIo::new(stream);
        let svc = proxy_service.clone();

        // Hand the connection to hyper; auto-detect h1/h2 on this socket
        tokio::spawn(async move {
            let _ = Builder::new(TokioExecutor::new())
                .serve_connection(io, svc)
                .await;
        });
    }
}
