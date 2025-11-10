pub mod config;
pub mod error;

use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Bytes;
use hyper::body::Incoming;
use hyper::service::Service;
use hyper::{Request, Response};
use shared::http::run_http_service;
use std::pin::Pin;

#[derive(thiserror::Error, Debug)]
pub enum IngestRouterError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub async fn run(config: config::Config) -> Result<(), IngestRouterError> {
    let router_service = IngestRouterService {};
    let router_task = run_http_service(&config.listener.host, config.listener.port, router_service);
    router_task.await?;
    Ok(())
}

struct IngestRouterService {}

impl Service<Request<Incoming>> for IngestRouterService {
    type Response = Response<BoxBody<Bytes, Self::Error>>;
    type Error = IngestRouterError;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        println!("Received request: {:?}", req);
        Box::pin(async move {
            Ok(Response::new(
                Full::new("ok\n".into()).map_err(|e| match e {}).boxed(),
            ))
        })
    }
}
