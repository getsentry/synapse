use thiserror::Error;

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

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Locator error: {0}")]
    LocatorError(#[from] locator::locator::LocatorError),

    #[error("Backup route provider error: {0}")]
    BackupError(#[from] locator::backup_routes::BackupError),

    #[error("Locator client error: {0}")]
    LocatorClientError(#[from] locator::client::ClientError),
}
