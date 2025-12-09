use thiserror::Error;

/// Result type alias for ingest-router operations
pub type Result<T, E = IngestRouterError> = std::result::Result<T, E>;

/// Errors that can occur during ingest-router operations
#[derive(Error, Debug)]
pub enum IngestRouterError {
    #[error("Failed to read request body: {0}")]
    RequestBodyError(String),

    #[error("Failed to read response body: {0}")]
    ResponseBodyError(String),

    #[error("No route matched for request")]
    NoRouteMatched,

    #[error("Upstream not found: {0}")]
    UpstreamNotFound(String),

    #[error("Upstream request failed for {0}: {1}")]
    UpstreamRequestFailed(String, String),

    #[error("Upstream timeout for {0}")]
    UpstreamTimeout(String),

    #[error("Response serialization error: {0}")]
    ResponseSerializationError(String),

    #[error("Hyper error: {0}")]
    HyperError(String),

    #[error("HTTP client error: {0}")]
    HttpClientError(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    /// Request failed with the original request data for recovery
    #[error("Request failed")]
    RequestFailedWithData {
        /// The request that failed
        request_json: serde_json::Value,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Locator client error: {0}")]
    LocatorClientError(#[from] locator::client::ClientError),
}
