use crate::api::utils::normalize_headers;
use crate::errors::IngestRouterError;
use crate::handler::{CellId, Handler, SplitMetadata};
use crate::locale::Cells;
use async_trait::async_trait;
use http::StatusCode;
use hyper::body::Bytes;
use hyper::{Request, Response};
use shared::http::make_error_response;

/// Health check handler for the `/api/0/relays/live/` endpoint.
///
/// This endpoint returns success if any one upstream is available.
/// Synapse should continue to operate even if one cell is down.
pub struct HealthHandler;

#[async_trait]
impl Handler for HealthHandler {
    async fn split_request(
        &self,
        request: Request<Bytes>,
        cells: &Cells,
    ) -> Result<(Vec<(CellId, Request<Bytes>)>, SplitMetadata), IngestRouterError> {
        let (mut parts, body) = request.into_parts();
        normalize_headers(&mut parts.headers, parts.version);

        // Send the health check request to all cells
        let cell_requests = cells
            .cell_list()
            .iter()
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
                    return response;
                }
                Ok(response) => {
                    tracing::warn!(
                        cell_id = %cell_id,
                        status = %response.status(),
                        "Health check failed with non-success status"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        cell_id = %cell_id,
                        error = %e,
                        "Health check request failed"
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
        let handler = HealthHandler;
        let cells = create_test_cells();

        let request = Request::builder()
            .method("GET")
            .uri("/api/0/relays/live/")
            .body(Bytes::new())
            .unwrap();

        let (cell_requests, _metadata) = handler.split_request(request, &cells).await.unwrap();

        assert_eq!(cell_requests.len(), 2);
        let cell_ids = cell_requests.iter().map(|(id, _)| id.as_str()).collect();
        assert!(cell_ids.contains(&"us1"));
        assert!(cell_ids.contains(&"us2"));
    }

    #[tokio::test]
    async fn test_merge_responses_one_success() {
        let handler = HealthHandler;

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
        let handler = HealthHandler;

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
