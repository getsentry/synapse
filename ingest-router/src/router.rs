use crate::api::health::HealthHandler;
use crate::api::project_config::ProjectConfigsHandler;
use crate::api::register_challenge::RegisterChallenge;
use crate::api::register_response::RegisterResponse;
use crate::config::{CellConfig, HandlerAction, Route};
use crate::handler::Handler;
use crate::locale::{Cells, Locales};
use hyper::Request;
use locator::client::Locator;
use std::collections::HashMap;
use std::sync::Arc;

/// Router that matches incoming requests against configured routes
pub struct Router {
    routes: Arc<Vec<Route>>,
    action_to_handler: HashMap<HandlerAction, Arc<dyn Handler>>,
    locales_to_cells: Locales,
}

impl Router {
    /// Creates a new router with the given routes
    pub fn new(
        routes: Vec<Route>,
        locales: HashMap<String, Vec<CellConfig>>,
        locator: Locator,
    ) -> Self {
        let action_to_handler = HashMap::from([
            (
                HandlerAction::RelayProjectConfigs,
                Arc::new(ProjectConfigsHandler::new(locator)) as Arc<dyn Handler>,
            ),
            (HandlerAction::Health, Arc::new(HealthHandler {})),
            (
                HandlerAction::RegisterChallenge,
                Arc::new(RegisterChallenge {}),
            ),
            (
                HandlerAction::RegisterResponse,
                Arc::new(RegisterResponse {}),
            ),
        ]);

        Self {
            routes: Arc::new(routes),
            action_to_handler,
            locales_to_cells: Locales::new(locales),
        }
    }

    /// Finds the first route that matches the incoming request
    pub fn resolve<B>(&self, req: &Request<B>) -> Option<(Arc<dyn Handler>, Cells)> {
        self.routes
            .iter()
            .find(|route| self.matches_route(req, route))
            .and_then(|route| {
                let cells = self.locales_to_cells.get_cells(&route.locale)?;
                let handler = self.action_to_handler.get(&route.action)?.clone();
                Some((handler, cells))
            })
    }

    /// Checks if a request matches a route's criteria
    fn matches_route<B>(&self, req: &Request<B>, route: &Route) -> bool {
        // Match host if specified
        if let Some(expected_host) = &route.r#match.host {
            let req_host = req
                .headers()
                .get(hyper::header::HOST)
                .and_then(|h| h.to_str().ok());

            match req_host {
                Some(host) => {
                    // Strip port if present for comparison
                    let host_without_port = host.split(':').next().unwrap_or(host);
                    if host_without_port != expected_host {
                        return false;
                    }
                }
                None => return false,
            }
        }

        // Match path if specified
        if let Some(expected_path) = &route.r#match.path
            && req.uri().path() != expected_path
        {
            return false;
        }

        // Match method if specified
        if let Some(expected_method) = &route.r#match.method
            && expected_method != req.method()
        {
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HttpMethod, Match, Route};
    use crate::testutils::get_mock_provider;
    use http_body_util::Empty;
    use http_body_util::{BodyExt, combinators::BoxBody};
    use hyper::body::Bytes;
    use hyper::header::HOST;
    use hyper::{Method, Request};
    use locator::config::LocatorDataType;
    use locator::locator::Locator as LocatorService;
    use url::Url;

    async fn test_router(routes: Option<Vec<Route>>) -> Router {
        let default_routes = vec![
            Route {
                r#match: Match {
                    host: Some("api.example.com".into()),
                    path: Some("/api/test".into()),
                    method: Some(HttpMethod::Post),
                },
                action: HandlerAction::RelayProjectConfigs,
                locale: "us".to_string(),
            },
            Route {
                r#match: Match {
                    host: None,
                    path: Some("/health".to_string()),
                    method: Some(HttpMethod::Get),
                },
                action: HandlerAction::Health,
                locale: "us".to_string(),
            },
        ];

        let routes = routes.unwrap_or(default_routes);

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

        Router::new(routes, locales, locator)
    }

    fn test_request(
        method: Method,
        path: &str,
        host: Option<&str>,
    ) -> Request<BoxBody<Bytes, std::convert::Infallible>> {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(h) = host {
            builder = builder.header(HOST, h);
        }
        builder
            .body(
                Empty::<Bytes>::new()
                    .map_err(|never| match never {})
                    .boxed(),
            )
            .unwrap()
    }

    #[tokio::test]
    async fn test_route_matching() {
        let router = test_router(None).await;

        // Should match first route
        let req = test_request(Method::POST, "/api/test", Some("api.example.com"));
        let (handler, _cells) = router.resolve(&req).unwrap();
        assert!(handler.type_name().contains("ProjectConfigsHandler"));

        // Should match second route
        let req = test_request(Method::GET, "/health", None);
        let (handler, _cells) = router.resolve(&req).unwrap();
        assert!(handler.type_name().contains("HealthHandler"));
    }

    #[tokio::test]
    async fn test_no_route_matched() {
        let router = test_router(None).await;

        let req = test_request(Method::GET, "/different", None);
        assert!(router.resolve(&req).is_none());
    }

    #[tokio::test]
    async fn test_host_matching_with_port() {
        let routes = vec![Route {
            r#match: Match {
                host: Some("api.example.com".to_string()),
                path: None,
                method: None,
            },
            action: HandlerAction::RelayProjectConfigs,
            locale: "us".to_string(),
        }];

        let router = test_router(Some(routes)).await;

        // Should strip port and match
        let req = test_request(Method::GET, "/test", Some("api.example.com:8080"));
        let (handler, _cells) = router.resolve(&req).unwrap();
        assert!(handler.type_name().contains("ProjectConfigsHandler"));
    }

    #[tokio::test]
    async fn test_method_matching() {
        let routes = vec![Route {
            r#match: Match {
                host: None,
                path: Some("/api/test".to_string()),
                method: Some(HttpMethod::Post),
            },
            action: HandlerAction::RelayProjectConfigs,
            locale: "us".to_string(),
        }];

        let router = test_router(Some(routes)).await;

        // POST should match
        let req = test_request(Method::POST, "/api/test", None);
        let (handler, _cells) = router.resolve(&req).unwrap();
        assert!(handler.type_name().contains("ProjectConfigsHandler"));

        // GET should not match
        let req = test_request(Method::GET, "/api/test", None);
        assert!(router.resolve(&req).is_none());
    }
}
