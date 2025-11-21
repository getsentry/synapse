pub mod config;
pub mod errors;
pub mod http;
pub mod locale;
pub mod relay_project_config_handler;
pub mod router;

use crate::config::{CellConfig, Config, Route};
use crate::errors::IngestRouterError;
use crate::relay_project_config_handler::RelayProjectConfigsHandler;
use crate::router::Router;
use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper::body::Incoming;
use hyper::service::Service;
use hyper::{Request, Response};
use shared::http::run_http_service;
use std::collections::HashMap;
use std::pin::Pin;

pub async fn run(config: Config) -> Result<(), errors::IngestRouterError> {
    let router_service = IngestRouterService::new(config.routes.clone(), config.locales.clone());
    let router_task = run_http_service(&config.listener.host, config.listener.port, router_service);
    router_task.await?;
    Ok(())
}

#[derive(Clone)]
struct IngestRouterService {
    router: Router,
}

impl IngestRouterService {
    fn new(routes: Vec<Route>, locales: HashMap<String, HashMap<String, CellConfig>>) -> Self {
        let handler = RelayProjectConfigsHandler::new(locales);
        Self {
            router: Router::new(routes, handler),
        }
    }
}

impl Service<Request<Incoming>> for IngestRouterService {
    type Response = Response<BoxBody<Bytes, IngestRouterError>>;
    type Error = IngestRouterError;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let router = self.router.clone();
        Box::pin(async move { router.route(req).await })
    }
}
