use http_body_util::BodyExt;
use hyper::body::Bytes;
use hyper::{Request, Response};
use hyper_util::client::legacy::Client;
use shared::http::{add_via_header, filter_hop_by_hop};
use std::time::Duration;
use tokio::time::timeout;

use crate::errors::IngestRouterError;

/// Send a request to a single upstream with configurable timeout
///
/// This function handles the complete request/response cycle including:
/// - Building the full URI by combining the upstream base URL with the request path
/// - Filtering hop-by-hop headers in both directions (request and response)
/// - Adding Via headers to indicate the request passed through this service
/// - Collecting the entire response body into bytes
///
/// # Timeout Behavior
///
/// The `timeout_secs` parameter applies to the entire request/response cycle, including:
/// - Establishing the connection
/// - Sending the request
/// - Receiving response headers
/// - **Collecting the complete response body**
///
/// **Important**: This function is NOT suitable for:
/// - Server-Sent Events (SSE)
/// - Long-lived streaming connections
pub async fn send_to_upstream<C, B>(
    client: &Client<C, B>,
    upstream_url: &url::Url,
    request: Request<B>,
    timeout_secs: u64,
) -> Result<Response<Bytes>, IngestRouterError>
where
    C: hyper_util::client::legacy::connect::Connect + Clone + Send + Sync + 'static,
    B: hyper::body::Body + Send + Unpin + 'static,
    B::Data: Send,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    // Use host as identifier for error messages
    let upstream_identifier = upstream_url.host_str().unwrap_or(upstream_url.as_str());

    // Build the full upstream URI by combining base URL with request path
    let path_and_query = match request.uri().path_and_query() {
        Some(pq) => pq.as_str(),
        None => {
            return Err(IngestRouterError::InternalError(
                "Request URI missing path and query".to_string(),
            ));
        }
    };

    let mut url = upstream_url.clone();
    if let Some((path, query)) = path_and_query.split_once('?') {
        url.set_path(path);
        url.set_query(Some(query));
    } else {
        url.set_path(path_and_query);
    }
    let upstream_uri = url.to_string();

    // Build request to send to upstream with modified URI and filtered headers
    let (mut parts, body) = request.into_parts();
    let request_version = parts.version;
    filter_hop_by_hop(&mut parts.headers, request_version);
    add_via_header(&mut parts.headers, request_version);

    let mut req_builder = Request::builder()
        .method(parts.method)
        .uri(upstream_uri)
        .version(parts.version);

    for (name, value) in parts.headers.iter() {
        req_builder = req_builder.header(name, value);
    }

    let upstream_request = req_builder
        .body(body)
        .map_err(|e| IngestRouterError::InternalError(format!("Failed to build request: {e}")))?;

    // Send request with timeout
    let response = timeout(
        Duration::from_secs(timeout_secs),
        client.request(upstream_request),
    )
    .await
    // First map_err: Handle timeout - tokio::time::timeout returns Err if duration elapsed
    .map_err(|_| IngestRouterError::UpstreamTimeout(upstream_identifier.to_string()))?
    // Second map_err: Handle HTTP client errors (connection failures, network errors, etc.)
    .map_err(|e| {
        IngestRouterError::UpstreamRequestFailed(upstream_identifier.to_string(), e.to_string())
    })?;

    // Collect response body bytes and filter hop-by-hop headers
    let (mut parts, body) = response.into_parts();
    let response_version = parts.version;
    filter_hop_by_hop(&mut parts.headers, response_version);
    add_via_header(&mut parts.headers, response_version);

    let body_bytes = body
        .collect()
        .await
        .map(|collected| collected.to_bytes())
        .map_err(|e| IngestRouterError::ResponseBodyError(e.to_string()))?;

    Ok(Response::from_parts(parts, body_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::Full;
    use hyper::service::service_fn;
    use hyper_util::client::legacy::connect::HttpConnector;
    use hyper_util::rt::TokioExecutor;
    use std::convert::Infallible;
    use tokio::net::TcpListener;

    // Simple echo server that returns the request body
    async fn echo_handler(
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        let (parts, body) = req.into_parts();

        // Collect the request body
        let body_bytes = body
            .collect()
            .await
            .map(|collected| collected.to_bytes())
            .unwrap_or_else(|_| Bytes::new());

        // Echo back the request body with original headers
        let mut response = Response::new(Full::new(body_bytes));
        *response.headers_mut() = parts.headers;

        Ok(response)
    }

    async fn start_test_server() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind to address");

        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let io = hyper_util::rt::TokioIo::new(stream);

                tokio::spawn(async move {
                    if let Err(err) =
                        hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                            .serve_connection(io, service_fn(echo_handler))
                            .await
                    {
                        eprintln!("Error serving connection: {:?}", err);
                    }
                });
            }
        });

        // Give the server a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        port
    }

    #[tokio::test]
    async fn test_send_to_upstream_success() {
        let port = start_test_server().await;

        let conn = HttpConnector::new();
        let client: Client<HttpConnector, Full<Bytes>> =
            Client::builder(TokioExecutor::new()).build(conn);

        let upstream_url =
            url::Url::parse(&format!("http://127.0.0.1:{}", port)).expect("Failed to parse URL");

        let content = b"hello world";
        let request = Request::builder()
            .uri("http://example.com/test?foo=bar")
            .header("connection", "keep-alive") // Should be filtered out
            .header("x-custom", "test-value")
            .method("POST")
            .body(Full::new(Bytes::from_static(content)))
            .unwrap();

        let response = send_to_upstream(&client, &upstream_url, request, 5).await;

        assert!(response.is_ok());
        let response = response.unwrap();
        assert_eq!(response.status(), 200);

        // Verify body was collected
        assert_eq!(response.body().as_ref(), content);

        // Via header should be added
        assert!(response.headers().contains_key("via"));

        // Hop-by-hop headers should be filtered out
        assert!(!response.headers().contains_key("connection"));
    }

    #[tokio::test]
    async fn test_send_to_upstream_timeout() {
        let conn = HttpConnector::new();
        let client: Client<HttpConnector, Full<Bytes>> =
            Client::builder(TokioExecutor::new()).build(conn);

        // Use a non-routable IP to trigger timeout
        let upstream_url = url::Url::parse("http://192.0.2.1:9999").expect("Failed to parse URL");

        let request = Request::builder()
            .uri("http://example.com/test")
            .body(Full::new(Bytes::from_static(b"test")))
            .unwrap();

        let result = send_to_upstream(&client, &upstream_url, request, 1).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IngestRouterError::UpstreamTimeout(_)
        ));
    }
}
