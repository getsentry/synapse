use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Bytes;
use hyper::{Response, StatusCode};
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

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl IngestRouterError {
    /// Returns the appropriate HTTP status code for this error
    pub fn status_code(&self) -> StatusCode {
        match self {
            IngestRouterError::RequestBodyError(_) => StatusCode::BAD_REQUEST,
            IngestRouterError::ResponseBodyError(_) => StatusCode::BAD_GATEWAY,
            IngestRouterError::UpstreamRequestFailed(_, _) => StatusCode::BAD_GATEWAY,
            IngestRouterError::UpstreamTimeout(_) => StatusCode::GATEWAY_TIMEOUT,
            IngestRouterError::ResponseSerializationError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            IngestRouterError::ResponseBuildError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            IngestRouterError::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            IngestRouterError::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            IngestRouterError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Converts this error into an HTTP response
    pub fn into_response(self) -> Response<BoxBody<Bytes, IngestRouterError>> {
        let status = self.status_code();
        let body = format!("{}\n", self);

        Response::builder()
            .status(status)
            .body(Full::new(Bytes::from(body)).map_err(|e| match e {}).boxed())
            .unwrap_or_else(|_| {
                // Fallback if response building fails
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(
                        Full::new(Bytes::from("Internal server error\n"))
                            .map_err(|e| match e {})
                            .boxed(),
                    )
                    .unwrap()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_error_into_response() {
        use http_body_util::BodyExt;

        // Test that errors convert to proper HTTP responses
        let error = IngestRouterError::ServiceUnavailable("Test unavailable".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        // Verify body contains error message
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("Test unavailable"));
    }
}
