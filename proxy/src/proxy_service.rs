use crate::config;
use crate::errors::ProxyError;
use crate::locator::Locator;
use crate::resolvers::Resolvers;
use crate::route_actions::RouteActions;
use crate::upstreams::Upstreams;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::service::Service as HyperService;
use hyper::{Request, Response, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use std::future::Future;
use std::pin::Pin;

pub struct ProxyService<B>
where
    B: BodyExt<Data = Bytes> + Send + Sync + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
    B: Unpin,
{
    #[allow(dead_code)]
    client: Client<HttpConnector, B>,
    pub route_actions: RouteActions,
    upstreams: Upstreams,
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

        let upstreams = Upstreams::try_new(upstream_config)?;

        let resolvers = Resolvers::try_new(locator)?;

        Ok(Self {
            client,
            route_actions,
            upstreams,
            resolvers,
        })
    }

    fn make_error_response(status_code: StatusCode) -> Response<BoxBody<Bytes, hyper::Error>> {
        let message = status_code
            .canonical_reason()
            .unwrap_or("an error occurred");

        Response::builder()
            .status(status_code)
            .body(Full::new(message.into()).map_err(|e| match e {}).boxed())
            .unwrap()
    }
}

impl<B> HyperService<Request<B>> for ProxyService<B>
where
    B: BodyExt<Data = Bytes> + Send + Sync + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
    B: Unpin,
{
    type Response = Response<BoxBody<Bytes, hyper::Error>>;
    type Error = hyper::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&self, request: Request<B>) -> Self::Future {
        let route = self.route_actions.resolve(&request);

        println!("Resolved route: {:?}", route);

        let upstream_name = route.and_then(|r| match r.action {
            config::Action::Static { to } => Some(to.as_str()),
            config::Action::Dynamic {
                resolver,
                cell_to_upstream,
                default,
                ..
            } => self
                .resolvers
                .resolve(resolver, cell_to_upstream, r.params)
                .ok()
                .or(default.as_deref()),
        });

        let upstream = upstream_name.and_then(|u| self.upstreams.get(u));

        println!("Resolved upstream: {:?}", upstream);

        match upstream {
            Some(u) => {
                // Build target URI: keep path+query, swap scheme+authority to upstream_base
                let (mut parts, body) = request.into_parts();

                // Compose new URI: {scheme}://{authority}{path_and_query}
                let path_and_query = parts
                    .uri
                    .path_and_query()
                    .map(|pq| pq.as_str())
                    .unwrap_or("/");

                let new_uri = http::Uri::builder()
                    .scheme(u.scheme.clone())
                    .authority(u.authority.clone())
                    .path_and_query(path_and_query)
                    .build()
                    .unwrap();

                parts.uri = new_uri;

                let outbound_request = Request::from_parts(parts, body);

                let client = self.client.clone();

                // TODO: handle headers properly
                // - rewrite host header
                // - strip hop-by-hop headers

                Box::pin(async move {
                    match client.request(outbound_request).await {
                        Ok(response) => {
                            println!("response headers: {:?}", response.headers());
                            // Convert the response body to BoxBody
                            let (parts, body) = response.into_parts();
                            let boxed_body = body.map_err(|e| e).boxed();
                            Ok(Response::from_parts(parts, boxed_body))
                        }
                        Err(e) => {
                            eprintln!("Upstream request failed: {}", e);
                            Ok(Self::make_error_response(StatusCode::BAD_GATEWAY))
                        }
                    }
                })
            }
            None => {
                // No upstream found, return 404
                Box::pin(async move { Ok(Self::make_error_response(StatusCode::NOT_FOUND)) })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command};

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
        use hyper::service::Service as HyperService;
        use std::time::Duration;

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
            admin_listener: config::Listener {
                host: "127.0.0.1".to_string(),
                port: 8081,
            },
            locator: config::Locator {
                r#type: config::LocatorType::Url {
                    url: "something".to_string(),
                },
            },
        };

        let locator = Locator::new_from_config(config.locator.clone());

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
        println!("response headers: {:?}", response.headers());
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
