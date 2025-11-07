use http_body_util::combinators::BoxBody;
use hyper::body::{Bytes, Incoming};
use hyper::service::Service;
use hyper::{Request, Response};
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use hyper_util::server::conn::auto::Builder;
use std::sync::Arc;
use tokio::net::TcpListener;

pub async fn run_http_service<S, E>(host: &str, port: u16, service: S) -> Result<(), E>
where
    S: Service<Request<Incoming>, Response = Response<BoxBody<Bytes, E>>, Error = E>
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
    E: From<std::io::Error> + std::error::Error + Send + Sync + 'static,
{
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
