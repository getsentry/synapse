use crate::errors::{IngestRouterError, Result};
use crate::executor::UpstreamTaskExecutor;
use crate::locale::Cells;
use async_trait::async_trait;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Bytes;
use hyper::{Request, Response};
use serde::{Serialize, de::DeserializeOwned};

pub type CellId = String;

/// Request type that can be sent to upstreams
pub trait UpstreamRequest: Clone + Serialize + DeserializeOwned + Send + Sync + 'static {}

/// Response type that can be received from upstreams
pub trait UpstreamResponse: Serialize + DeserializeOwned + Send + 'static {}

// Blanket implementations
impl<T> UpstreamRequest for T where T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static {}

impl<T> UpstreamResponse for T where T: Serialize + DeserializeOwned + Send + 'static {}

/// Handler for endpoints that split requests across cells and merge results
///
/// The handler implements endpoint-specific logic:
/// - How to split a request into per-cell requests
/// - How to merge results from multiple cells
/// ```
#[async_trait]
pub trait Handler<Req, Res>: Send + Sync
where
    Req: Serialize + DeserializeOwned + Send,
    Res: Serialize + DeserializeOwned + Send,
{
    /// Metadata that flows from split_requests to merge_results
    ///
    /// This allows passing data from the split phase to the merge phase.
    /// Some use cases:
    /// - Pending keys that couldn't be routed (e.g., `Vec<PublicKey>`)
    type SplitMetadata: Send;

    /// Split one request into multiple per-cell requests
    ///
    /// This method routes the request data to appropriate cells and builds
    /// per-cell requests that will be sent to upstreams.
    async fn split_requests(
        &self,
        request: Req,
        cells: &Cells,
    ) -> Result<(Vec<(CellId, Req)>, Self::SplitMetadata)>;

    /// Merge results from multiple cells into a single response
    ///
    /// This method combines responses from successful cells, handles failures,
    /// and incorporates metadata from the split phase.
    ///
    /// Results are pre-separated and sorted by cell priority (highest first).
    fn merge_results(
        &self,
        successful: Vec<(CellId, Res)>,
        failed: Vec<(CellId, IngestRouterError)>,
        metadata: Self::SplitMetadata,
    ) -> Res;
}

#[allow(dead_code)]
pub async fn handle_request<H, Req, Res>(
    handler: &H,
    executor: &UpstreamTaskExecutor,
    request: Request<Full<Bytes>>,
    cells: &Cells,
) -> Result<Response<BoxBody<Bytes, IngestRouterError>>>
where
    H: Handler<Req, Res>,
    Req: UpstreamRequest,
    Res: UpstreamResponse,
{
    // Parse request body
    let (parts, body) = request.into_parts();
    let body_bytes = body
        .collect()
        .await
        .map_err(|e| IngestRouterError::InternalError(format!("Failed to read request body: {e}")))?
        .to_bytes();

    let req: Req = serde_json::from_slice(&body_bytes)
        .map_err(|e| IngestRouterError::InternalError(format!("Failed to parse request: {e}")))?;

    // Split requests across cells
    let (cell_requests, metadata) = handler.split_requests(req, cells).await?;

    // Add upstream URLs to cell requests
    let cell_requests_with_urls: Vec<(CellId, url::Url, Req)> = cell_requests
        .into_iter()
        .map(|(cell_id, req)| {
            let upstream = cells.cell_to_upstreams.get(&cell_id).ok_or_else(|| {
                IngestRouterError::InternalError(format!("Cell {cell_id} not found"))
            })?;
            Ok((cell_id, upstream.sentry_url.clone(), req))
        })
        .collect::<Result<Vec<_>, IngestRouterError>>()?;

    // Execute requests in parallel
    let base_request = Request::from_parts(parts, ());
    let mut results = executor
        .execute(cell_requests_with_urls, &base_request)
        .await;

    // Sort results by cell priority (highest priority first)
    // This allows merge_results to simply take the first successful result
    results.sort_by_key(|result| {
        let cell_id = match result {
            Ok((id, _)) => id,
            Err((id, _)) => id,
        };
        cells
            .cell_list
            .iter()
            .position(|c| c == cell_id)
            .unwrap_or(usize::MAX)
    });

    // Separate successful and failed results
    let (successful, failed): (Vec<_>, Vec<_>) = results.into_iter().partition(|r| r.is_ok());

    let successful: Vec<_> = successful.into_iter().filter_map(|r| r.ok()).collect();
    // Convert RequestFailed<Req> to IngestRouterError at the boundary
    let failed: Vec<_> = failed
        .into_iter()
        .filter_map(|r| r.err())
        .map(|(cell_id, req_failed)| (cell_id, req_failed.into()))
        .collect();

    // Merge results (now separated and in priority order)
    let response = handler.merge_results(successful, failed, metadata);

    // Serialize response
    let response_body = serde_json::to_vec(&response).map_err(|e| {
        IngestRouterError::InternalError(format!("Failed to serialize response: {e}"))
    })?;

    Response::builder()
        .status(200)
        .header("Content-Type", "application/json")
        .body(
            Full::new(Bytes::from(response_body))
                .map_err(|e| match e {})
                .boxed(),
        )
        .map_err(|e| IngestRouterError::InternalError(format!("Failed to build response: {e}")))
}
