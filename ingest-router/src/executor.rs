use crate::config::RelayTimeouts;
use crate::errors::IngestRouterError;
use crate::handler::{CellId, Handler};
use crate::http::send_to_upstream;
use crate::locale::Cells;
use http::StatusCode;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::{Request, Response};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use shared::http::make_error_response;
use std::collections::HashSet;
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
        request: Request<Bytes>,
        cells: Cells,
    ) -> Response<Bytes> {
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
        requests: Vec<(CellId, Request<Bytes>)>,
        cells: Cells,
    ) -> Vec<(CellId, Result<Response<Bytes>, IngestRouterError>)> {
        let mut join_set = JoinSet::new();

        let mut pending_cells = HashSet::new();

        // Spawn requests for each cell
        for (cell_id, request) in requests {
            let cells = cells.clone();
            let client = self.client.clone();
            let timeout_secs = self.timeouts.http_timeout_secs;

            pending_cells.insert(cell_id.clone());
            join_set.spawn(async move {
                let result = send_to_cell(&client, &cell_id, request, &cells, timeout_secs).await;
                (cell_id, result)
            });
        }

        let mut results = Vec::new();

        // Use the longer initial timeout for the first result
        let initial_timeout = sleep(Duration::from_secs(self.timeouts.task_initial_timeout_secs));

        tokio::select! {
            _ = initial_timeout => {},
            join_result = join_set.join_next() => {
                match join_result {
                    Some(Ok((cell_id, result))) => {
                        pending_cells.remove(&cell_id);
                        results.push((cell_id, result));
                    }
                    Some(Err(e)) => tracing::error!("Task panicked: {}", e),
                    // The join set is empty -- this should never happen
                    None => return results,
                }
            }
        }

        // Use the shorter subsequent timeout for any remaining results
        let timeout = sleep(Duration::from_secs(
            self.timeouts.task_subsequent_timeout_secs,
        ));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                _ = &mut timeout => {
                    break;
                },
                join_result = join_set.join_next() => {
                    match join_result {
                        Some(Ok((cell_id, result))) => {
                            pending_cells.remove(&cell_id);
                            results.push((cell_id, result));
                        },
                        Some(Err(e)) => tracing::error!("Task panicked: {}", e),
                        // No more tasks
                        None => break,
                    }
                }
            }
        }

        // Add all remaining pending cells to results
        for cell_id in pending_cells.drain() {
            results.push((
                cell_id.clone(),
                Err(IngestRouterError::UpstreamTimeout(cell_id)),
            ));
        }

        results
    }
}

/// Send a request to a specific cell's upstream.
async fn send_to_cell(
    client: &Client<HttpConnector, Full<Bytes>>,
    cell_id: &str,
    request: Request<Bytes>,
    cells: &Cells,
    timeout_secs: u64,
) -> Result<Response<Bytes>, IngestRouterError> {
    // Look up the upstream for this cell
    let upstream = cells
        .cell_to_upstreams()
        .get(cell_id)
        .ok_or_else(|| IngestRouterError::InternalError(format!("Unknown cell: {}", cell_id)))?;

    // Wrap Bytes in Full for the HTTP client
    let (parts, body) = request.into_parts();
    let request = Request::from_parts(parts, Full::new(body));

    // Send to upstream (using relay_url) - returns Response<Bytes>
    send_to_upstream(client, &upstream.relay_url, request, timeout_secs).await
}
