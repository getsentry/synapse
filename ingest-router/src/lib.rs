pub mod api;
pub mod config;
pub mod errors;
mod executor;
pub mod handler;
pub mod http;
pub mod locale;
pub mod project_config;
pub mod router;

#[cfg(test)]
mod testutils;

use crate::errors::IngestRouterError;
use http_body_util::{BodyExt, Full};
use hyper::StatusCode;
use hyper::body::Bytes;
use hyper::service::Service;
use hyper::{Request, Response};
use locator::client::Locator;
use shared::http::{make_error_response, run_http_service};
use std::pin::Pin;

pub async fn run(config: config::Config) -> Result<(), IngestRouterError> {
    let locator = Locator::new(config.locator.to_client_config()).await?;

    let ingest_router_service = IngestRouterService::new(
        router::Router::new(config.routes, config.locales, locator),
        config.relay_timeouts,
    );

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
    executor: executor::Executor,
}

impl IngestRouterService {
    pub fn new(router: router::Router, timeouts: config::RelayTimeouts) -> Self {
        let executor = executor::Executor::new(timeouts);
        Self { router, executor }
    }
}

impl<B> Service<Request<B>> for IngestRouterService
where
    B: BodyExt<Data = Bytes> + Send + Sync + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
    B: Unpin,
{
    type Response = Response<Full<Bytes>>;
    type Error = IngestRouterError;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&self, req: Request<B>) -> Self::Future {
        let maybe_handler = self.router.resolve(&req);

        match maybe_handler {
            Some((handler, cells)) => {
                let (parts, body) = req.into_parts();
                let executor = self.executor.clone();

                Box::pin(async move {
                    let body_bytes = match body.collect().await {
                        Ok(c) => c.to_bytes(),
                        Err(_) => {
                            return Ok(make_error_response(StatusCode::BAD_REQUEST).map(Full::new));
                        }
                    };
                    let request = Request::from_parts(parts, body_bytes);
                    let response = executor.execute(handler, request, cells).await;
                    Ok(response.map(Full::new))
                })
            }
            None => {
                Box::pin(
                    async move { Ok(make_error_response(StatusCode::BAD_REQUEST).map(Full::new)) },
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::utils::deserialize_body;
    use crate::config::{CellConfig, HandlerAction, HttpMethod, Match, Route};
    use crate::project_config::protocol::ProjectConfigsResponse;
    use crate::testutils::create_test_locator;
    use hyper::Method;
    use hyper::header::HOST;
    use std::collections::HashMap;
    use std::process::{Child, Command};
    use url::Url;

    struct TestServer {
        child: Child,
    }

    impl TestServer {
        fn spawn() -> std::io::Result<Self> {
            let child = Command::new("python")
                .arg("../scripts/mock_relay_api.py")
                .spawn()?;

            Ok(Self { child })
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[tokio::test]
    async fn test_ingest_router() {
        let _relay_server = TestServer::spawn().expect("Failed to spawn test server");

        let routes_config = vec![
            Route {
                r#match: Match {
                    host: Some("us.sentry.io".to_string()),
                    path: Some("/api/0/relays/projectconfigs/".to_string()),
                    method: Some(HttpMethod::Post),
                },
                action: HandlerAction::RelayProjectConfigs,
                locale: "us".to_string(),
            },
            Route {
                r#match: Match {
                    host: Some("us.sentry.io".to_string()),
                    path: Some("/api/0/relays/live/".to_string()),
                    method: Some(HttpMethod::Get),
                },
                action: HandlerAction::Health,
                locale: "us".to_string(),
            },
        ];

        let locales = HashMap::from([(
            "us".to_string(),
            vec![CellConfig {
                id: "us1".to_string(),
                sentry_url: Url::parse("https://sentry.io/us1").unwrap(),
                relay_url: Url::parse("http://localhost:8000").unwrap(),
            }],
        )]);

        let locator = create_test_locator(HashMap::from([(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            "us1".to_string(),
        )]))
        .await;

        let service = IngestRouterService::new(
            router::Router::new(routes_config, locales, locator),
            config::RelayTimeouts {
                http_timeout_secs: 5000,
                task_initial_timeout_secs: 10000,
                task_subsequent_timeout_secs: 10000,
            },
        );

        // Project configs request
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/0/relays/projectconfigs/")
            .header(HOST, "us.sentry.io")
            .body(Full::new(Bytes::from(
                r#"{"publicKeys": ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"], "global": 1}"#,
            )))
            .unwrap();

        let response = service.call(request).await.unwrap();

        let (parts, body) = response.into_parts();

        assert_eq!(parts.status, 200);

        // Convert BoxBody to Bytes for deserialize_body
        let body_bytes = body.collect().await.unwrap().to_bytes();
        let parsed: ProjectConfigsResponse = deserialize_body(body_bytes).unwrap();
        assert_eq!(parsed.project_configs.len(), 1);
        assert_eq!(parsed.pending_keys.len(), 0);
        assert_eq!(parsed.extra_fields.len(), 2);

        // Healthcheck
        let request = Request::builder()
            .method(Method::GET)
            .uri("/api/0/relays/live/")
            .header(HOST, "us.sentry.io")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let response = service.call(request).await.unwrap();
        assert_eq!(response.status(), 200);
    }
}
