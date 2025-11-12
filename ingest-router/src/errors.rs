use thiserror::Error;

/// Errors that can occur during ingest-router operations
#[derive(Error, Debug)]
pub enum IngestRouterError {
    #[error("Failed to read request body: {0}")]
    RequestBodyError(String),

    #[error("Failed to read response body: {0}")]
    ResponseBodyError(String),

    #[error("Upstream request failed for {0}: {1}")]
    UpstreamRequestFailed(String, String),

    #[error("Upstream timeout for {0}")]
    UpstreamTimeout(String),

    #[error("Failed to serialize response: {0}")]
    ResponseSerializationError(String),

    #[error("Failed to build response: {0}")]
    ResponseBuildError(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
