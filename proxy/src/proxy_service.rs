use crate::config;
use crate::route_actions::RouteActions;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;
use hyper::service::Service as HyperService;
use hyper::{Request, Response};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use std::future::Future;
use std::pin::Pin;

#[allow(dead_code)]
pub struct ProxyService {
    config: config::Config,
    client: Client<HttpConnector, Incoming>,
    route_actions: RouteActions,
}

impl ProxyService {
    pub fn new(config: config::Config) -> Self {
        let conn = HttpConnector::new();
        let client: Client<_, Incoming> = Client::builder(TokioExecutor::new())
            .http2_adaptive_window(true)
            .build(conn);

        let route_actions = RouteActions::new(config.routes.clone());

        Self {
            config,
            client,
            route_actions,
        }
    }
}

impl HyperService<Request<Incoming>> for ProxyService {
    type Response = Response<BoxBody<Bytes, hyper::Error>>;
    type Error = hyper::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&self, _req: Request<Incoming>) -> Self::Future {
        unimplemented!();
    }
}
