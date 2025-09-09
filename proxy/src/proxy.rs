use std::sync::Arc;
use hyper::{Body, Client, Request, Response, Server};
use hyper::service::{make_service_fn, service_fn};
use std::convert::Infallible;
use std::net::SocketAddr;

use crate::config::Config;
use crate::rules_engine::{RulesEngine, IncomingRequest};

pub struct ProxyServer {
    rules_engine: Arc<RulesEngine>,
    client: Client<hyper::client::HttpConnector>,
}

impl ProxyServer {
    pub fn new(config: Config) -> Self {
        let rules_engine = Arc::new(RulesEngine::new(config.routes));
        let client = Client::new();

        Self {
            rules_engine,
            client,
        }
    }

    pub async fn run(&self, addr: SocketAddr) -> Result<(), hyper::Error> {
        let proxy_server = Arc::new(self);
        
        let make_svc = make_service_fn(move |_conn| {
            let proxy_server = Arc::clone(&proxy_server);
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    let proxy_server = Arc::clone(&proxy_server);
                    async move { proxy_server.handle_request(req).await }
                }))
            }
        });

        let server = Server::bind(&addr).serve(make_svc);
        server.await
    }

    async fn handle_request(&self, req: Request<Body>) -> Result<Response<Body>, Infallible> {
        let host = req.headers()
            .get("host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        
        let path = req.uri().path();
        
        let incoming_request = IncomingRequest { host, path };
        
        match self.rules_engine.find_destination(&incoming_request) {
            Some(destination) => {
                match self.forward_request(req, &destination).await {
                    Ok(response) => Ok(response),
                    Err(_) => Ok(Response::builder()
                        .status(502)
                        .body(Body::from("Bad Gateway"))
                        .unwrap()),
                }
            }
            None => Ok(Response::builder()
                .status(404)
                .body(Body::from("Not Found"))
                .unwrap()),
        }
    }

    async fn forward_request(&self, mut req: Request<Body>, destination: &str) -> Result<Response<Body>, hyper::Error> {
        // Modify request URI to point to destination
        let uri = format!("{}{}", destination, req.uri().path_and_query().map(|pq| pq.as_str()).unwrap_or(""));
        *req.uri_mut() = uri.parse().unwrap();
        
        // Remove hop-by-hop headers
        req.headers_mut().remove("connection");
        req.headers_mut().remove("proxy-connection");
        req.headers_mut().remove("te");
        req.headers_mut().remove("trailer");
        req.headers_mut().remove("transfer-encoding");
        req.headers_mut().remove("upgrade");
        
        // Forward the request
        self.client.request(req).await
    }
}