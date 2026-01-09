use crate::api::utils::normalize_headers;
use crate::errors::IngestRouterError;
use crate::handler::{CellId, ExecutionMode, Handler, SplitMetadata};
use crate::locale::Cells;
use async_trait::async_trait;
use http::StatusCode;
use hyper::body::Bytes;
use hyper::{Request, Response};
use shared::http::make_error_response;

/// Handler for endpoints that can be routed to any cell.
///
/// This handler clones the request to all cells and returns the first
/// successful response (failover mode). It's suitable for endpoints where:
/// - Any cell can handle the request
/// - The Sentry upstream handles cross-cell coordination via outboxes
/// - Success from one cell is sufficient -- synapse operates even if one cell is down
///
///
/// # Used for:
///
/// - `GET /api/0/relays/live/` - Health check
/// - `POST /api/0/relays/register/challenge/` - Relay registration challenge
/// - `POST /api/0/relays/register/response/` - Relay registration response
pub struct AnyCellHandler {
    name: &'static str,
}

impl AnyCellHandler {
    pub fn new(name: &'static str) -> Self {
        Self { name }
    }
}

#[async_trait]
impl Handler for AnyCellHandler {
    fn name(&self) -> &'static str {
        self.name
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Failover
    }

    async fn split_request(
        &self,
        request: Request<Bytes>,
        cells: &Cells,
    ) -> Result<(Vec<(CellId, Request<Bytes>)>, SplitMetadata), IngestRouterError> {
        let (mut parts, body) = request.into_parts();
        normalize_headers(&mut parts.headers, parts.version);

        // Send the request to all cells
        let cell_requests = cells
            .cell_list()
            .map(|cell_id| {
                let req = Request::from_parts(parts.clone(), body.clone());
                (cell_id.clone(), req)
            })
            .collect();

        Ok((cell_requests, Box::new(())))
    }

    async fn merge_responses(
        &self,
        responses: Vec<(CellId, Result<Response<Bytes>, IngestRouterError>)>,
        _metadata: SplitMetadata,
    ) -> Response<Bytes> {
        // Return the first successful response
        for (cell_id, result) in responses {
            match result {
                Ok(response) if response.status().is_success() => {
                    let (mut parts, body) = response.into_parts();
                    normalize_headers(&mut parts.headers, parts.version);
                    return Response::from_parts(parts, body);
                }
                Ok(response) => {
                    tracing::warn!(
                        cell_id = %cell_id,
                        status = %response.status(),
                        "{} failed with non-success status",
                        self.name
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        cell_id = %cell_id,
                        error = %e,
                        "{} request failed",
                        self.name
                    );
                }
            }
        }

        // All cells failed - return service unavailable
        make_error_response(StatusCode::SERVICE_UNAVAILABLE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;
    use crate::locale::Locales;
    use std::collections::HashMap;
    use url::Url;

    fn create_test_cells() -> Cells {
        let locales = HashMap::from([(
            "us".to_string(),
            vec![
                CellConfig {
                    id: "us1".to_string(),
                    sentry_url: Url::parse("http://sentry-us1:8080").unwrap(),
                    relay_url: Url::parse("http://relay-us1:8090").unwrap(),
                },
                CellConfig {
                    id: "us2".to_string(),
                    sentry_url: Url::parse("http://sentry-us2:8080").unwrap(),
                    relay_url: Url::parse("http://relay-us2:8090").unwrap(),
                },
            ],
        )]);
        Locales::new(locales).get_cells("us").unwrap()
    }

    #[tokio::test]
    async fn test_split_request_sends_to_all_cells() {
        let handler = AnyCellHandler::new("HealthCheck");
        let cells = create_test_cells();

        let request = Request::builder()
            .method("GET")
            .uri("/api/0/relays/live/")
            .body(Bytes::new())
            .unwrap();

        let (cell_requests, _metadata) = handler.split_request(request, &cells).await.unwrap();

        assert_eq!(cell_requests.len(), 2);
        let cell_ids: Vec<_> = cell_requests.iter().map(|(id, _)| id.as_str()).collect();
        assert!(cell_ids.contains(&"us1"));
        assert!(cell_ids.contains(&"us2"));
    }

    #[tokio::test]
    async fn test_merge_responses_one_success() {
        let handler = AnyCellHandler::new("HealthCheck");

        let success_response = Response::builder()
            .status(StatusCode::OK)
            .body(Bytes::from(r#"{"is_healthy":true}"#))
            .unwrap();

        let responses = vec![
            (
                "us1".to_string(),
                Err(IngestRouterError::UpstreamTimeout("us1".to_string())),
            ),
            ("us2".to_string(), Ok(success_response)),
        ];

        let merged = handler.merge_responses(responses, Box::new(())).await;

        assert_eq!(merged.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_merge_responses_all_failed() {
        let handler = AnyCellHandler::new("HealthCheck");

        let responses = vec![
            (
                "us1".to_string(),
                Err(IngestRouterError::UpstreamTimeout("us1".to_string())),
            ),
            (
                "us2".to_string(),
                Err(IngestRouterError::UpstreamTimeout("us2".to_string())),
            ),
        ];

        let merged = handler.merge_responses(responses, Box::new(())).await;

        assert_eq!(merged.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
