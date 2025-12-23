pub mod api;
pub mod config;
pub mod errors;
pub mod handler;
pub mod http;
pub mod locale;
pub mod project_config;
pub mod router;

#[cfg(test)]
mod testutils;

use crate::errors::IngestRouterError;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::StatusCode;
use hyper::body::Bytes;
use hyper::service::Service;
use hyper::{Request, Response};
use locator::client::Locator;
use shared::http::make_error_response;
use shared::http::run_http_service;
use std::pin::Pin;

pub async fn run(config: config::Config) -> Result<(), IngestRouterError> {
    let locator = Locator::new(config.locator.to_client_config()).await?;

    let ingest_router_service = IngestRouterService {
        router: router::Router::new(config.routes, config.locales, locator),
    };
    let router_task = run_http_service(
        &config.listener.host,
        config.listener.port,
        ingest_router_service,
    );
    router_task.await?;
    Ok(())
}

struct IngestRouterService {
    router: router::Router,
}

impl<B> Service<Request<B>> for IngestRouterService
where
    B: BodyExt<Data = Bytes> + Send + Sync + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
    B: Unpin,
{
    type Response = Response<BoxBody<Bytes, Self::Error>>;
    type Error = IngestRouterError;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&self, req: Request<B>) -> Self::Future {
        let maybe_handler = self.router.resolve(&req);

        match maybe_handler {
            Some((handler, cells)) => {
                // Convert Request<B> to Request<HandlerBody>
                let (parts, body) = req.into_parts();
                let handler_body = body
                    .map_err(|e| IngestRouterError::RequestBodyError(e.to_string()))
                    .boxed();
                let handler_req = Request::from_parts(parts, handler_body);

                // TODO: Placeholder response
                Box::pin(async move {
                    let (split, _metadata) = handler.split_request(handler_req, &cells).await?;

                    for (cell_id, req) in split {
                        println!("Cell: {}, URI: {}", cell_id, req.uri());
                    }

                    Ok(Response::new(
                        Full::new("ok\n".into()).map_err(|e| match e {}).boxed(),
                    ))
                })
            }
            None => Box::pin(async move { Ok(make_error_response(StatusCode::BAD_REQUEST)) }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HandlerAction, HttpMethod, Match, Route};
    use hyper::Method;
    use hyper::body::Bytes;
    use hyper::header::HOST;

    use crate::config::CellConfig;
    use locator::config::LocatorDataType;
    use locator::locator::Locator as LocatorService;
    use std::collections::HashMap;
    use url::Url;

    use crate::testutils::get_mock_provider;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_ingest_router() {
        let routes_config = vec![Route {
            r#match: Match {
                host: Some("us.sentry.io".to_string()),
                path: Some("/api/0/relays/projectconfigs/".to_string()),
                method: Some(HttpMethod::Post),
            },
            action: HandlerAction::RelayProjectConfigs,
            locale: "us".to_string(),
        }];

        let locales = HashMap::from([(
            "us".to_string(),
            vec![CellConfig {
                id: "us1".to_string(),
                sentry_url: Url::parse("https://sentry.io/us1").unwrap(),
                relay_url: Url::parse("https://relay.io/us1").unwrap(),
            }],
        )]);

        let (_dir, provider) = get_mock_provider().await;
        let locator_service = LocatorService::new(
            LocatorDataType::ProjectKey,
            "http://control-plane-url".to_string(),
            Arc::new(provider),
            None,
        );
        let locator = Locator::from_in_process_service(locator_service);

        let service = IngestRouterService {
            router: router::Router::new(routes_config, locales, locator),
        };

        // Project configs request
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/0/relays/projectconfigs/")
            .header(HOST, "us.sentry.io")
            .body(
                Full::new(Bytes::from(r#"{"publicKeys": ["test-key"]}"#))
                    .map_err(|e| match e {})
                    .boxed(),
            )
            .unwrap();

        let response = service.call(request).await.unwrap();

        // TODO: call the scripts/mock_relay_api.py server and validate the response

        assert_eq!(response.status(), 200);
    }
}
