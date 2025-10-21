use crate::config;
use crate::errors::ProxyError;
use crate::resolvers::Resolvers;
use crate::route_actions::RouteActions;
use crate::locator::Locator;
use crate::upstreams::Upstreams;
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

pub struct ProxyService {
    #[allow(dead_code)]
    client: Client<HttpConnector, Incoming>,
    pub route_actions: RouteActions,
    upstreams: Upstreams,
    resolvers: Resolvers,
}

impl ProxyService {
    pub fn try_new(config: config::Config, locator: Locator) -> Result<Self, ProxyError> {
        let conn = HttpConnector::new();
        let client: Client<_, Incoming> = Client::builder(TokioExecutor::new())
            .http2_adaptive_window(true)
            .build(conn);

        let route_actions = RouteActions::try_new(config.routes.clone())?;

        let upstreams = Upstreams::try_new(config.upstreams)?;

        let resolvers = Resolvers::try_new(locator)?;

        Ok(Self {
            client,
            route_actions,
            upstreams,
            resolvers,
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

        let upstream_name = route.and_then(|r| match r.action {
            config::Action::Static { to } => Some(to.as_str()),
            config::Action::Dynamic {
                resolver,
                cell_to_upstream,
                default,
                ..
            } => {
                self.resolvers
                    .resolve(resolver, &cell_to_upstream, r.params)
                    .ok()
                    .or(default.as_deref())
            }
        });

        let upstream = upstream_name.and_then(|u| self.upstreams.get(u));

        println!("Resolved upstream: {:?}", upstream);

        // TODO: Actually proxy the request not just return 404
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
