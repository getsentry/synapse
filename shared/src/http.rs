use http::Version;
use http::header::{
    CONNECTION, HeaderMap, HeaderName, HeaderValue, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, TE,
    TRAILER, TRANSFER_ENCODING, UPGRADE, VIA,
};
use http_body_util::BodyExt;
use http_body_util::combinators::BoxBody;
use hyper::body::{Bytes, Incoming};
use hyper::service::Service;
use hyper::{Request, Response};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use hyper_util::server::conn::auto::Builder;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;

pub async fn run_http_service<S, E>(host: &str, port: u16, service: S) -> Result<(), E>
where
    S: Service<Request<Incoming>, Response = Response<BoxBody<Bytes, E>>, Error = E>
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
    E: From<std::io::Error> + std::error::Error + Send + Sync + 'static,
{
    let listener = TcpListener::bind(format!("{host}:{port}")).await?;
    let service_arc = Arc::new(service);

    loop {
        let (stream, _peer_addr) = listener.accept().await?;
        let _ = stream.set_nodelay(true);
        let io = TokioIo::new(stream);
        let svc = service_arc.clone();

        // Hand the connection to hyper; auto-detect h1/h2 on this socket
        tokio::spawn(async move {
            let _ = Builder::new(TokioExecutor::new())
                .serve_connection(io, svc)
                .await;
        });
    }
}

static HOP_BY_HOP_NAMES: &[HeaderName] = &[
    CONNECTION,
    TRANSFER_ENCODING,
    TE,
    TRAILER,
    UPGRADE,
    PROXY_AUTHORIZATION,
    PROXY_AUTHENTICATE,
];

fn is_http1(v: Version) -> bool {
    matches!(v, Version::HTTP_09 | Version::HTTP_10 | Version::HTTP_11)
}

/// Adds a Via header to indicate the request/response passed through this proxy.
/// Appends to existing if Via is already present.
/// Should be applied to proxied requests in both directions
pub fn add_via_header(headers: &mut HeaderMap, version: Version) {
    let proxy_name = "synapse";

    let version_str = match version {
        Version::HTTP_09 => "0.9",
        Version::HTTP_10 => "1.0",
        Version::HTTP_11 => "1.1",
        Version::HTTP_2 => "2",
        Version::HTTP_3 => "3",
        _ => {
            tracing::warn!(
                "Unknown/future HTTP version, skipping Via header: {:?}",
                version
            );
            return;
        }
    };

    let via_value = format!("{} {}", version_str, proxy_name);

    if let Some(existing) = headers.get(VIA) {
        if let Ok(existing_str) = existing.to_str() {
            let combined = format!("{}, {}", existing_str, via_value);
            if let Ok(new_value) = HeaderValue::from_str(&combined) {
                headers.insert(VIA, new_value);
            }
        }
    } else if let Ok(new_value) = HeaderValue::from_str(&via_value) {
        headers.insert(VIA, new_value);
    }
}

// For HTTP/1.x connections, hop-by-hop headers are removed before forwarding:
// - standard hop-by-hop headers
// - any extra headers listed in the Connection header value
// - keep-alive header for HTTP/0.9 and HTTP/1.0 only
//
// HTTP/2 and HTTP/3 don't use hop-by-hop headers, so no filtering is performed.
/// Should be applied to proxied requests in both directions
pub fn filter_hop_by_hop(headers: &mut HeaderMap, version: Version) -> &mut HeaderMap {
    if !is_http1(version) {
        return headers;
    }

    // Parse the Connection header to find additional headers to drop
    let mut extra_drops = Vec::new();
    if let Some(connection) = headers.get(CONNECTION)
        && let Ok(s) = connection.to_str()
    {
        for token in s.split(',').map(|t| t.trim()).filter(|t| !t.is_empty()) {
            if let Ok(name) = HeaderName::from_bytes(token.as_bytes()) {
                extra_drops.push(name);
            }
        }
    }

    // Remove standard hop-by-hop headers
    for name in HOP_BY_HOP_NAMES {
        headers.remove(name);
    }

    // Remove headers listed in the Connection header
    for name in extra_drops {
        headers.remove(&name);
    }

    // For HTTP/0.9 and HTTP/1.0, also remove keep-alive
    if matches!(version, Version::HTTP_09 | Version::HTTP_10) {
        headers.remove(HeaderName::from_static("keep-alive"));
    }

    headers
}

/// Send a request to a single upstream with configurable timeout
pub async fn send_to_upstream<C, B, E>(
    client: &Client<C, B>,
    upstream_url: &url::Url,
    request: Request<B>,
    timeout_secs: u64,
) -> Result<Response<Bytes>, E>
where
    C: hyper_util::client::legacy::connect::Connect + Clone + Send + Sync + 'static,
    B: hyper::body::Body + Send + Unpin + 'static,
    B::Data: Send,
    B::Error: std::error::Error + Send + Sync + 'static,
    E: From<UpstreamError>,
{
    // Use host as identifier for error messages
    let upstream_identifier = upstream_url.host_str().unwrap_or(upstream_url.as_str());

    // Build the full upstream URI by combining base URL with request path
    let path_and_query = request
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

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
    filter_hop_by_hop(&mut parts.headers, parts.version);

    let mut req_builder = Request::builder()
        .method(parts.method)
        .uri(upstream_uri)
        .version(parts.version);

    for (name, value) in parts.headers.iter() {
        req_builder = req_builder.header(name, value);
    }

    let upstream_request = req_builder.body(body).map_err(|e| {
        E::from(UpstreamError::InternalError(format!(
            "Failed to build request: {e}"
        )))
    })?;

    // Send request with timeout
    let response = timeout(
        Duration::from_secs(timeout_secs),
        client.request(upstream_request),
    )
    .await
    .map_err(|_| E::from(UpstreamError::Timeout(upstream_identifier.to_string())))?
    .map_err(|e| {
        E::from(UpstreamError::RequestFailed(
            upstream_identifier.to_string(),
            e.to_string(),
        ))
    })?;

    // Collect response body bytes
    let (parts, body) = response.into_parts();
    let body_bytes = body
        .collect()
        .await
        .map(|collected| collected.to_bytes())
        .map_err(|e| E::from(UpstreamError::ResponseBodyError(e.to_string())))?;

    Ok(Response::from_parts(parts, body_bytes))
}

/// Errors that can occur during upstream operations
#[derive(Debug)]
pub enum UpstreamError {
    RequestBodyError(String),
    ResponseBodyError(String),
    Timeout(String),
    RequestFailed(String, String),
    InternalError(String),
}

impl std::fmt::Display for UpstreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpstreamError::RequestBodyError(s) => write!(f, "Failed to read request body: {}", s),
            UpstreamError::ResponseBodyError(s) => write!(f, "Failed to read response body: {}", s),
            UpstreamError::Timeout(name) => write!(f, "Upstream timeout for {}", name),
            UpstreamError::RequestFailed(name, err) => {
                write!(f, "Upstream request failed for {}: {}", name, err)
            }
            UpstreamError::InternalError(s) => write!(f, "Internal error: {}", s),
        }
    }
}

impl std::error::Error for UpstreamError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_headers() {
        use http::header::{CONNECTION, CONTENT_TYPE, HeaderMap, HeaderValue};

        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, HeaderValue::from_static("keep-alive, custom"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("cusTOM", HeaderValue::from_static("some-value"));
        headers.insert("keep-alive", HeaderValue::from_static("timeout=5"));

        let filtered = filter_hop_by_hop(&mut headers, Version::HTTP_11);

        assert_eq!(filtered.len(), 1);
        // should remain
        assert_eq!(
            filtered.get(CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json"))
        );
        // should be removed
        assert!(filtered.get(CONNECTION).is_none());
        // listed in the Connection header value
        assert!(filtered.get("keep-alive").is_none());
        // Case-insensitive match with "cusTOM"
        assert!(filtered.get("custom").is_none());
    }
}
