use crate::config::RelayTimeouts;
use crate::errors::IngestRouterError;
use crate::handler::{CellId, Handler, HandlerBody};
use crate::http::send_to_upstream;
use crate::locale::Cells;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Request, Response};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use shared::http::make_error_response;
use std::sync::Arc;
use tokio::task::JoinSet;
use tokio::time::{Duration, sleep};

#[derive(Clone)]
pub struct Executor {
    client: Client<HttpConnector, Full<Bytes>>,
    timeouts: RelayTimeouts,
}

impl Executor {
    pub fn new(timeouts: RelayTimeouts) -> Self {
        let client = Client::builder(TokioExecutor::new()).build(HttpConnector::new());
        Self { client, timeouts }
    }

    // Splits, executes, and merges the responses using the provided handler.
    pub async fn execute(
        &self,
        handler: Arc<dyn Handler>,
        request: Request<HandlerBody>,
        cells: Cells,
    ) -> Response<HandlerBody> {
        let (split_requests, metadata) = match handler.split_request(request, &cells).await {
            Ok(result) => result,
            Err(_e) => return make_error_response(StatusCode::INTERNAL_SERVER_ERROR),
        };

        let results = self.execute_parallel(split_requests, cells).await;

        handler.merge_responses(results, metadata).await
    }

    /// Execute split requests in parallel against their cell upstreams
    async fn execute_parallel(
        &self,
        requests: Vec<(CellId, Request<HandlerBody>)>,
        cells: Cells,
    ) -> Vec<(CellId, Result<Response<HandlerBody>, IngestRouterError>)> {
        let mut join_set = JoinSet::new();

        // Spawn requests for each cell
        for (cell_id, request) in requests {
            let cells = cells.clone();
            let client = self.client.clone();
            let timeout_secs = self.timeouts.http_timeout_secs;

            join_set.spawn(async move {
                let result = send_to_cell(&client, &cell_id, request, &cells, timeout_secs).await;
                (cell_id, result)
            });
        }

        // Collect results with timeout
        let mut results = Vec::new();

        // TODO: Use task_initial_timeout_secs for first result, then task_subsequent_timeout_secs
        // for remaining results. Currently using http_timeout_secs for the entire collection.
        let timeout = sleep(Duration::from_secs(self.timeouts.http_timeout_secs));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                // TODO: add missing cells to results if we hit timeout before they returned
                _ = &mut timeout => break,
                join_result = join_set.join_next() => {
                    match join_result {
                        Some(Ok(result)) => results.push(result),
                        // TODO: panicked task should be added to the results vec as error
                        Some(Err(e)) => tracing::error!("Task panicked: {}", e),
                        None => break,
                    }
                }
            }
        }

        results
    }
}

/// Send a request to a specific cell's upstream
/// TODO: simplify body types so these conversions are not needed - consider converting to
/// Bytes at the boundary and using bytes only throughout the handlers.
async fn send_to_cell(
    client: &Client<HttpConnector, Full<Bytes>>,
    cell_id: &str,
    request: Request<HandlerBody>,
    cells: &Cells,
    timeout_secs: u64,
) -> Result<Response<HandlerBody>, IngestRouterError> {
    // Look up the upstream for this cell
    let upstream = cells
        .cell_to_upstreams()
        .get(cell_id)
        .ok_or_else(|| IngestRouterError::InternalError(format!("Unknown cell: {}", cell_id)))?;

    // Convert HandlerBody to Full<Bytes> for the HTTP client
    let (parts, body) = request.into_parts();
    let body_bytes = body
        .collect()
        .await
        .map_err(|e| IngestRouterError::RequestBodyError(e.to_string()))?
        .to_bytes();

    let request = Request::from_parts(parts, Full::new(body_bytes));

    // Send to upstream (using relay_url)
    let response = send_to_upstream(client, &upstream.relay_url, request, timeout_secs).await?;

    // Convert Response<Bytes> back to Response<HandlerBody>
    let (parts, body_bytes) = response.into_parts();
    let handler_body: HandlerBody = Full::new(body_bytes).map_err(|e| match e {}).boxed();

    Ok(Response::from_parts(parts, handler_body))
}
