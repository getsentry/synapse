use crate::config;
use crate::errors::ProxyError;
use crate::route_actions::RouteActions;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::Service as HyperService;
use hyper::{Request, Response, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use std::future::Future;
use std::pin::Pin;

#[allow(dead_code)]
pub struct ProxyService {
    config: config::Config,
    client: Client<HttpConnector, Incoming>,
    pub route_actions: RouteActions,
}

impl ProxyService {
    pub fn try_new(config: config::Config) -> Result<Self, ProxyError> {
        let conn = HttpConnector::new();
        let client: Client<_, Incoming> = Client::builder(TokioExecutor::new())
            .http2_adaptive_window(true)
            .build(conn);

        let route_actions = RouteActions::try_new(config.routes.clone())?;

        Ok(Self {
            config,
            client,
            route_actions,
        })
    }
}

impl HyperService<Request<Incoming>> for ProxyService {
    type Response = Response<BoxBody<Bytes, hyper::Error>>;
    type Error = hyper::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&self, request: Request<Incoming>) -> Self::Future {
        let route = self.route_actions.resolve(&request);

        println!("Resolved route: {:?}", route);
        Box::pin(async move {
            let res = Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(
                    Full::new("not found\n".into())
                        .map_err(|e| match e {})
                        .boxed(),
                )
                .unwrap();
            Ok(res)
        })
    }
}
