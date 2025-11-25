use crate::config::{HandlerAction, Route};
use crate::errors::IngestRouterError;
use crate::relay_project_config_handler::RelayProjectConfigsHandler;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Bytes;
use hyper::{Request, Response, StatusCode};
use std::sync::Arc;

/// Router that matches incoming requests against configured routes
#[derive(Clone)]
pub struct Router {
    routes: Arc<Vec<Route>>,
    handler: Arc<RelayProjectConfigsHandler>,
}

impl Router {
    /// Creates a new router with the given routes and handler
    pub fn new(routes: Vec<Route>, handler: RelayProjectConfigsHandler) -> Self {
        Self {
            routes: Arc::new(routes),
            handler: Arc::new(handler),
        }
    }

    /// Routes an incoming request to the appropriate handler
    pub async fn route<B>(
        &self,
        req: Request<B>,
    ) -> Result<Response<BoxBody<Bytes, IngestRouterError>>, IngestRouterError>
    where
        B: hyper::body::Body + Send + 'static,
        B::Data: Send,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        // Find a matching route
        match self.find_matching_route(&req) {
            Some(action) => {
                tracing::debug!(action = ?action, "Matched route");
                // Convert errors to HTTP responses with proper status codes
                match self.handle_action(req, action).await {
                    Ok(response) => Ok(response),
                    Err(e) => {
                        tracing::error!(error = %e, "Handler error");
                        Ok(e.into_response())
                    }
                }
            }
            None => {
                tracing::warn!(
                    method = %req.method(),
                    path = %req.uri().path(),
                    "No route matched"
                );
                self.handle_no_route()
            }
        }
    }

    /// Finds the first route that matches the incoming request
    fn find_matching_route<B>(&self, req: &Request<B>) -> Option<&HandlerAction> {
        self.routes
            .iter()
            .find(|route| self.matches_route(req, route))
            .map(|route| &route.action)
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

    /// Handles a matched action by calling the appropriate handler
    async fn handle_action<B>(
        &self,
        req: Request<B>,
        action: &HandlerAction,
    ) -> Result<Response<BoxBody<Bytes, IngestRouterError>>, IngestRouterError>
    where
        B: hyper::body::Body + Send + 'static,
        B::Data: Send,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        match action {
            HandlerAction::RelayProjectConfigs(args) => {
                self.handler.handle(&args.locale, req).await
            }
        }
    }

    /// Handles an unmatched request
    fn handle_no_route(
        &self,
    ) -> Result<Response<BoxBody<Bytes, IngestRouterError>>, IngestRouterError> {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(
                Full::new("No route matched\n".into())
                    .map_err(|e| match e {})
                    .boxed(),
            )
            .map_err(|e| IngestRouterError::InternalError(format!("Failed to build response: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HttpMethod, Match, RelayProjectConfigsArgs, Route};
    use hyper::body::Bytes;
    use hyper::header::HOST;
    use hyper::{Method, Request};

    fn test_router(routes: Vec<Route>) -> Router {
        use crate::config::{CellConfig, RelayTimeouts};
        use std::collections::HashMap;
        use url::Url;

        // Create test locales
        let mut locales = HashMap::new();
        locales.insert(
            "us".to_string(),
            vec![CellConfig {
                name: "us-cell-1".to_string(),
                relay_url: Url::parse("http://us-relay.example.com:8080").unwrap(),
                sentry_url: Url::parse("http://us-sentry.example.com:8080").unwrap(),
            }],
        );
        locales.insert(
            "de".to_string(),
            vec![CellConfig {
                name: "de-cell-1".to_string(),
                relay_url: Url::parse("http://de-relay.example.com:8080").unwrap(),
                sentry_url: Url::parse("http://de-sentry.example.com:8080").unwrap(),
            }],
        );
        locales.insert(
            "local".to_string(),
            vec![CellConfig {
                name: "local-cell".to_string(),
                relay_url: Url::parse("http://local-relay.example.com:8080").unwrap(),
                sentry_url: Url::parse("http://local-sentry.example.com:8080").unwrap(),
            }],
        );

        let handler = RelayProjectConfigsHandler::new(locales, RelayTimeouts::default());
        Router::new(routes, handler)
    }

    fn test_request(
        method: Method,
        path: &str,
        host: Option<&str>,
    ) -> Request<BoxBody<Bytes, std::convert::Infallible>> {
        // Create a valid relay project configs request body with empty publicKeys
        // so we don't need to contact upstreams in routing tests
        let request_body = serde_json::json!({
            "publicKeys": []
        });
        let body_str = serde_json::to_string(&request_body).unwrap();

        let mut builder = Request::builder().method(method).uri(path);
        if let Some(h) = host {
            builder = builder.header(HOST, h);
        }
        builder
            .body(
                http_body_util::Full::new(Bytes::from(body_str))
                    .map_err(|never| match never {})
                    .boxed(),
            )
            .unwrap()
    }

    fn test_route(
        host: Option<String>,
        path: Option<String>,
        method: Option<HttpMethod>,
        locale: &str,
    ) -> Route {
        Route {
            r#match: Match { host, path, method },
            action: HandlerAction::RelayProjectConfigs(RelayProjectConfigsArgs {
                locale: locale.to_string(),
            }),
        }
    }

    #[tokio::test]
    async fn test_route_matching() {
        let router = test_router(vec![
            test_route(
                Some("api.example.com".to_string()),
                Some("/api/test".to_string()),
                Some(HttpMethod::Post),
                "us",
            ),
            test_route(None, Some("/health".to_string()), None, "local"),
        ]);

        // Should match first route
        let req = test_request(Method::POST, "/api/test", Some("api.example.com"));
        let response = router.route(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Should match second route
        let req = test_request(Method::GET, "/health", None);
        let response = router.route(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_no_route_matched() {
        let router = test_router(vec![test_route(
            None,
            Some("/api/test".to_string()),
            None,
            "us",
        )]);

        let req = test_request(Method::GET, "/different", None);
        let response = router.route(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_host_matching_with_port() {
        let router = test_router(vec![test_route(
            Some("api.example.com".to_string()),
            None,
            None,
            "us",
        )]);

        // Should strip port and match
        let req = test_request(Method::GET, "/test", Some("api.example.com:8080"));
        let response = router.route(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_method_matching() {
        let router = test_router(vec![test_route(
            None,
            Some("/api/test".to_string()),
            Some(HttpMethod::Post),
            "us",
        )]);

        // POST should match
        let req = test_request(Method::POST, "/api/test", None);
        let response = router.route(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // GET should not match
        let req = test_request(Method::GET, "/api/test", None);
        let response = router.route(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_error_converted_to_response() {
        use std::collections::HashMap;

        // Create a router with a route that references a non-existent locale
        let handler = RelayProjectConfigsHandler::new(
            HashMap::new(), // Empty locales
            crate::config::RelayTimeouts::default(),
        );
        let routes = vec![test_route(
            None,
            Some("/test".to_string()),
            None,
            "nonexistent", // This locale doesn't exist
        )];
        let router = Router::new(routes, handler);

        // This should trigger an InternalError (locale not found) and return 500
        let req = test_request(Method::POST, "/test", None);
        let response = router.route(req).await.unwrap();

        // Verify the error was converted to a proper HTTP response
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
