use crate::config;
use crate::errors::ProxyError;
use crate::metrics_defs::{REQUEST_DURATION, REQUESTS_INFLIGHT};
use crate::resolvers::Resolvers;
use crate::route_actions::{RouteActions, RouteMatch};
use crate::upstreams::Upstreams;
use http_body_util::BodyExt;
use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper::service::Service;
use hyper::{Request, Response, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use locator::client::Locator;
use shared::http::{add_via_header, filter_hop_by_hop, make_boxed_error_response};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

// Counter for 1% metric sampling.
static REQUEST_COUNT: AtomicU64 = AtomicU64::new(0);

// Gauge for number of requests currently being processed.
static INFLIGHT: AtomicU64 = AtomicU64::new(0);

pub struct ProxyService<B>
where
    B: BodyExt<Data = Bytes> + Send + Sync + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
    B: Unpin,
{
    client: Client<HttpConnector, B>,
    pub route_actions: RouteActions,
    upstreams: Arc<Upstreams>,
    resolvers: Resolvers,
}

impl<B> ProxyService<B>
where
    B: BodyExt<Data = Bytes> + Send + Sync + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
    B: Unpin,
{
    pub fn try_new(
        locator: Locator,
        route_config: Vec<config::Route>,
        upstream_config: Vec<config::UpstreamConfig>,
    ) -> Result<Self, ProxyError> {
        let conn = HttpConnector::new();
        let client = Client::builder(TokioExecutor::new())
            .http2_adaptive_window(true)
            .build(conn);

        let route_actions = RouteActions::try_new(route_config)?;

        let upstreams = Arc::new(Upstreams::try_new(upstream_config)?);

        let resolvers = Resolvers::try_new(locator)?;

        Ok(Self {
            client,
            route_actions,
            upstreams,
            resolvers,
        })
    }
}

impl<B> Service<Request<B>> for ProxyService<B>
where
    B: BodyExt<Data = Bytes> + Send + Sync + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
    B: Unpin,
{
    type Response = Response<BoxBody<Bytes, ProxyError>>;
    type Error = ProxyError;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&self, request: Request<B>) -> Self::Future {
        let start = Instant::now();
        INFLIGHT.fetch_add(1, Ordering::Relaxed);

        let route = self.route_actions.resolve(&request);

        tracing::debug!("Resolved route: {route:?}");

        let upstreams = self.upstreams.clone();
        let resolvers = self.resolvers.clone();
        let client = self.client.clone();

        Box::pin(async move {
            let upstream_name: Option<String> = match route {
                Some(RouteMatch { action, params }) => match action {
                    config::Action::Static { to } => Some(to),
                    config::Action::Dynamic {
                        resolver,
                        cell_to_upstream,
                        default,
                        ..
                    } => resolvers
                        .resolve(&resolver, &cell_to_upstream, params)
                        .await
                        .ok()
                        .map(|s| s.to_string())
                        .or(default),
                },
                None => None,
            };

            let upstream = upstream_name.as_deref().and_then(|u| upstreams.get(u));

            tracing::debug!("Resolved upstream: {:?}", upstream);

            let response = match upstream {
                Some(u) => {
                    // Build target URI: keep path+query, swap scheme+authority to upstream_base
                    let (mut parts, body) = request.into_parts();

                    // Compose new URI: {scheme}://{authority}{path_and_query}
                    let path_and_query = parts.uri.path_and_query().map(|pq| pq.as_str());

                    if let Some(pq) = path_and_query {
                        match http::Uri::builder()
                            .scheme(u.scheme.clone())
                            .authority(u.authority.clone())
                            .path_and_query(pq)
                            .build()
                        {
                            Ok(new_uri) => {
                                parts.uri = new_uri;

                                // Filter hop-by-hop headers and add via header to request
                                let request_version = parts.version;
                                filter_hop_by_hop(&mut parts.headers, request_version);
                                add_via_header(&mut parts.headers, request_version);

                                let outbound_request = Request::from_parts(parts, body);

                                match client.request(outbound_request).await {
                                    Ok(mut response) => {
                                        // Filter hop-by-hop and add via to response from upstream
                                        let version = response.version();
                                        filter_hop_by_hop(response.headers_mut(), version);
                                        add_via_header(response.headers_mut(), version);

                                        // Convert the response body to BoxBody
                                        let (parts, body) = response.into_parts();
                                        let boxed_body = body.map_err(Into::into).boxed();
                                        Response::from_parts(parts, boxed_body)
                                    }
                                    Err(e) => {
                                        tracing::error!("Upstream request failed: {e}");
                                        make_boxed_error_response(StatusCode::BAD_GATEWAY)
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to build target URI: {e}");
                                make_boxed_error_response(StatusCode::INTERNAL_SERVER_ERROR)
                            }
                        }
                    } else {
                        tracing::warn!("Request URI missing path and query");
                        make_boxed_error_response(StatusCode::BAD_REQUEST)
                    }
                }
                None => {
                    // No upstream found, return 404
                    make_boxed_error_response(StatusCode::NOT_FOUND)
                }
            };

            // Record request metric (1% sample)
            if REQUEST_COUNT.fetch_add(1, Ordering::Relaxed).is_multiple_of(100) {
                metrics::histogram!(
                    REQUEST_DURATION.name,
                    "status" => response.status().as_u16().to_string(),
                    "upstream" => upstream_name.unwrap_or_else(|| "none".to_string()),
                )
                .record(start.elapsed().as_secs_f64());

                let inflight = INFLIGHT.load(Ordering::Relaxed);
                metrics::gauge!(REQUESTS_INFLIGHT.name).set(inflight as f64);
            }

            INFLIGHT.fetch_sub(1, Ordering::Relaxed);

            Ok(response)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::Full;
    use std::process::{Child, Command};
    use std::time::Duration;

    struct TestServer {
        child: Child,
    }

    impl TestServer {
        fn spawn() -> std::io::Result<Self> {
            let child = Command::new("python")
                .arg("../scripts/echo_server.py")
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
    async fn test_proxy_service() {
        // Start the test echo server
        let _server = TestServer::spawn().expect("Failed to spawn test server");

        // Wait for the server to start
        std::thread::sleep(Duration::from_millis(500));

        let config = config::Config {
            upstreams: vec![
                config::UpstreamConfig {
                    name: "upstream".to_string(),
                    url: "http://127.0.0.1:9000".to_string(),
                },
                config::UpstreamConfig {
                    name: "invalid_upstream".to_string(),
                    url: "http://256.256.256.256:9000".to_string(),
                },
            ],
            routes: vec![
                config::Route {
                    r#match: config::Match {
                        host: None,
                        path: Some("test".to_string()),
                    },
                    action: config::Action::Static {
                        to: "upstream".to_string(),
                    },
                },
                config::Route {
                    r#match: config::Match {
                        host: None,
                        path: None,
                    },
                    action: config::Action::Static {
                        to: "invalid_upstream".to_string(),
                    },
                },
            ],
            listener: config::Listener {
                host: "127.0.0.1".to_string(),
                port: 8080,
            },
            admin_listener: config::AdminListener {
                host: "127.0.0.1".to_string(),
                port: 8081,
            },
            locator: config::Locator {
                r#type: config::LocatorType::Url {
                    url: "something".to_string(),
                },
            },
        };

        let locator = Locator::new(config.locator.to_client_config())
            .await
            .unwrap();

        let service = ProxyService::try_new(locator, config.routes, config.upstreams)
            .expect("Failed to create proxy service");

        let content = b"hello world\n";

        // Successful request
        let request = Request::builder()
            .uri("http://example.com/test")
            .header("x-custom", "test")
            .method("GET")
            .body(Full::new(Bytes::from_static(content)))
            .unwrap();
        let response = service.call(request).await.expect("Request failed");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-custom").unwrap(), "test");
        assert_eq!(response.headers().get("host").unwrap(), "127.0.0.1:9000");
        tracing::debug!("response headers: {:?}", response.headers());
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body_bytes.as_ref(), content);

        // Invalid request (no upstream)
        let request = Request::builder()
            .uri("http://example.com/invalid")
            .header("x-custom", "test")
            .method("GET")
            .body(Full::new(Bytes::from_static(b"hello world")))
            .unwrap();
        let response = service.call(request).await.expect("Request failed");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }
}
