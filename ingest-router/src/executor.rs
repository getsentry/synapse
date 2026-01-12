use crate::config::RelayTimeouts;
use crate::errors::IngestRouterError;
use crate::handler::{CellId, ExecutionMode, Handler};
use crate::http::send_to_upstream;
use crate::locale::Cells;
use crate::metrics_defs::UPSTREAM_REQUEST_DURATION;
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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::task::JoinSet;
use tokio::time::{Duration, sleep};

// Counter for 1% metric sampling.
static UPSTREAM_REQUEST_COUNT: AtomicU64 = AtomicU64::new(0);

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

        let results = match handler.execution_mode() {
            ExecutionMode::Parallel => self.execute_parallel(split_requests, cells).await,
            ExecutionMode::Failover => self.execute_failover(split_requests, cells).await,
        };

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

    /// Execute requests sequentially in priority order, stopping on first success
    /// If no success, returns all failures
    async fn execute_failover(
        &self,
        requests: Vec<(CellId, Request<Bytes>)>,
        cells: Cells,
    ) -> Vec<(CellId, Result<Response<Bytes>, IngestRouterError>)> {
        let mut failures = Vec::new();

        for (cell_id, request) in requests {
            let result = send_to_cell(
                &self.client,
                &cell_id,
                request,
                &cells,
                self.timeouts.http_timeout_secs,
            )
            .await;

            match &result {
                Ok(response) if response.status().is_success() => {
                    return vec![(cell_id, result)];
                }
                Ok(response) => {
                    tracing::warn!(
                        cell_id = %cell_id,
                        status = %response.status(),
                        "Failover: non-success status, trying next cell"
                    );
                    failures.push((cell_id, result));
                }
                Err(e) => {
                    tracing::warn!(
                        cell_id = %cell_id,
                        error = %e,
                        "Failover: request failed, trying next cell"
                    );
                    failures.push((cell_id, result));
                }
            }
        }

        failures
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
        .get_upstream(cell_id)
        .ok_or_else(|| IngestRouterError::InternalError(format!("Unknown cell: {}", cell_id)))?;

    // Wrap Bytes in Full for the HTTP client
    let (parts, body) = request.into_parts();
    let request = Request::from_parts(parts, Full::new(body));

    // Send to upstream (using relay_url) - returns Response<Bytes>
    let start = Instant::now();
    let result = send_to_upstream(client, &upstream.relay_url, request, timeout_secs).await;

    // Record duration metric with status (1% sample)
    if UPSTREAM_REQUEST_COUNT
        .fetch_add(1, Ordering::Relaxed)
        .is_multiple_of(100)
    {
        let status = match &result {
            Ok(response) => response.status().as_u16().to_string(),
            Err(IngestRouterError::UpstreamTimeout(_)) => "timeout".to_string(),
            Err(_) => "error".to_string(),
        };
        metrics::histogram!(
            UPSTREAM_REQUEST_DURATION.name,
            "cell_id" => cell_id.to_string(),
            "status" => status,
        )
        .record(start.elapsed().as_secs_f64());
    }

    result
}
