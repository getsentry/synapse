//! Task execution for parallel upstream requests.

use crate::config::RelayTimeouts;
use crate::errors::IngestRouterError;
use crate::http::send_to_upstream;
use crate::locale::Cells;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::{Request, Response};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use std::collections::HashMap;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::Instant;

use super::merger::MergedResults;
use super::protocol::{ProjectConfigsRequest, ProjectConfigsResponse};
use super::splitter::SplitRequest;

/// Result from a single upstream task execution.
struct UpstreamTaskResult {
    /// The cell name this result came from
    cell_name: String,

    /// The public keys that were requested from this upstream.
    /// If the request fails, these keys will be added to the pending array
    public_keys: Vec<String>,

    /// The HTTP response from the upstream
    result: Result<Response<Bytes>, IngestRouterError>,
}

/// Collection of spawned upstream tasks with tracking metadata.
///
/// Used to track which public keys were sent to which tasks, so that
/// if a task fails or is aborted, we can add its keys to the pending array.
struct SpawnedTasks {
    /// The set of spawned async tasks making requests to upstreams
    join_set: JoinSet<UpstreamTaskResult>,

    /// Maps task IDs to their public keys for failure handling
    task_keys: HashMap<tokio::task::Id, Vec<String>>,
}

/// State machine for adaptive timeout collection strategy.
///
/// The collection process has two phases:
/// 1. **WaitingForFirst**: Initial phase waiting for first successful response
/// 2. **CollectingRemaining**: After first success, collecting remaining responses with shorter deadline
enum CollectionState {
    /// Initial phase: waiting for first successful response with long timeout.
    WaitingForFirst,

    /// Subsequent phase: first success received, collecting remaining with shorter timeout.
    /// Tracks when the first success occurred to calculate the subsequent deadline.
    CollectingRemaining { first_success_at: Instant },
}

impl CollectionState {
    /// Calculates the current deadline based on state and timeout configuration.
    fn current_deadline(&self, timeouts: &RelayTimeouts) -> Instant {
        match self {
            CollectionState::WaitingForFirst => {
                Instant::now() + Duration::from_secs(timeouts.task_initial_timeout_secs as u64)
            }
            CollectionState::CollectingRemaining { first_success_at } => {
                *first_success_at
                    + Duration::from_secs(timeouts.task_subsequent_timeout_secs as u64)
            }
        }
    }

    /// Transitions to the subsequent collection phase after first success.
    /// Only transitions if currently in WaitingForFirst state.
    fn transition_to_subsequent(&mut self) {
        if matches!(self, CollectionState::WaitingForFirst) {
            *self = CollectionState::CollectingRemaining {
                first_success_at: Instant::now(),
            };
        }
    }
}

/// Orchestrates parallel execution of upstream requests with adaptive timeout strategy.
///
/// This executor fans out requests to multiple upstream Sentry instances simultaneously,
/// collects their responses, and merges them. It implements a two-phase adaptive timeout
/// strategy to balance responsiveness with resilience:
///
/// 1. **Initial phase**: Wait up to `task_initial_timeout_secs` for the first upstream to respond
/// 2. **Subsequent phase**: Once first success occurs, give all remaining tasks `task_subsequent_timeout_secs` total to complete
///
/// This ensures fast cells aren't blocked by slow/failing cells, while still allowing
/// sufficient time for healthy upstreams to respond.
pub struct UpstreamTaskExecutor {
    /// HTTP client for making requests to upstream Sentry instances
    client: Client<HttpConnector, Full<Bytes>>,

    /// Timeout configuration for HTTP requests and task-level deadlines
    timeouts: RelayTimeouts,
}

impl UpstreamTaskExecutor {
    pub fn new(client: Client<HttpConnector, Full<Bytes>>, timeouts: RelayTimeouts) -> Self {
        Self { client, timeouts }
    }

    /// Spawns and collects results from all upstream tasks.
    pub async fn execute(
        &self,
        split_requests: Vec<SplitRequest>,
        base_request: &Request<()>,
        cells: &Cells,
    ) -> Result<MergedResults, IngestRouterError> {
        let spawned_tasks = self.spawn_upstream_tasks(split_requests, base_request)?;
        let merged = self.collect_results(spawned_tasks, cells).await;
        Ok(merged)
    }

    /// Spawns parallel tasks for all upstream requests.
    ///
    /// Returns spawned tasks with tracking metadata for failure handling.
    fn spawn_upstream_tasks(
        &self,
        split_requests: Vec<SplitRequest>,
        base_request: &Request<()>,
    ) -> Result<SpawnedTasks, IngestRouterError> {
        let mut join_set = JoinSet::new();
        let mut task_keys = HashMap::new();

        for split in split_requests {
            let request = self.build_upstream_request(&split.request, base_request)?;
            let public_keys_for_tracking = split.request.public_keys.clone();

            let client = self.client.clone();
            let sentry_url = split.upstream.sentry_url;
            let http_timeout = self.timeouts.http_timeout_secs as u64;
            let cell_name = split.cell_name;
            let public_keys = split.request.public_keys;

            let abort_handle = join_set.spawn(async move {
                let result = send_to_upstream(&client, &sentry_url, request, http_timeout).await;

                UpstreamTaskResult {
                    cell_name,
                    public_keys,
                    result,
                }
            });

            task_keys.insert(abort_handle.id(), public_keys_for_tracking);
        }

        Ok(SpawnedTasks {
            join_set,
            task_keys,
        })
    }

    /// Builds an HTTP request to send to an upstream Sentry instance.
    ///
    /// Copies method, URI, version, and headers from the original request,
    /// but replaces the body with the split request data.
    fn build_upstream_request(
        &self,
        split_request: &ProjectConfigsRequest,
        base_request: &Request<()>,
    ) -> Result<Request<Full<Bytes>>, IngestRouterError> {
        let request_body = split_request.to_bytes().map_err(|e| {
            IngestRouterError::InternalError(format!("Failed to serialize request: {e}"))
        })?;

        let mut req_builder = Request::builder()
            .method(base_request.method())
            .uri(base_request.uri())
            .version(base_request.version());

        for (name, value) in base_request.headers() {
            req_builder = req_builder.header(name, value);
        }

        req_builder.body(Full::new(request_body)).map_err(|e| {
            IngestRouterError::InternalError(format!("Failed to build HTTP request: {e}"))
        })
    }

    /// Collects results with adaptive timeout strategy using state machine.
    async fn collect_results(&self, spawned_tasks: SpawnedTasks, cells: &Cells) -> MergedResults {
        let SpawnedTasks {
            mut join_set,
            mut task_keys,
        } = spawned_tasks;
        let mut results = MergedResults::new();
        let mut extra_by_cell = HashMap::new();
        let mut headers_by_cell = HashMap::new();

        let mut state = CollectionState::WaitingForFirst;
        let deadline = tokio::time::sleep_until(state.current_deadline(&self.timeouts));
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                Some(join_result) = join_set.join_next_with_id() => {
                    if let Some((cell_name, extra, headers, request_succeeded)) =
                        self.handle_task_completion(join_result, &mut task_keys, &mut results)
                    {
                        extra_by_cell.insert(cell_name.clone(), extra);
                        headers_by_cell.insert(cell_name, headers);

                        if request_succeeded && matches!(state, CollectionState::WaitingForFirst) {
                            state.transition_to_subsequent();
                            deadline.as_mut().reset(state.current_deadline(&self.timeouts));
                        }
                    }
                }
                _ = &mut deadline => {
                    let remaining = join_set.len();
                    match state {
                        CollectionState::WaitingForFirst => {
                            tracing::error!(
                                "Initial timeout reached with no successful responses, aborting {} tasks",
                                remaining
                            );
                        }
                        CollectionState::CollectingRemaining { .. } => {
                            tracing::debug!(
                                "Subsequent deadline reached after first success, aborting {} remaining tasks",
                                remaining
                            );
                        }
                    }
                    join_set.abort_all();
                    break;
                }
                else => {
                    tracing::debug!("All tasks completed");
                    break;
                }
            }
        }

        while let Some(result) = join_set.join_next_with_id().await {
            let task_id = match result {
                Ok((id, _)) => id,
                Err(e) => e.id(),
            };
            if let Some(keys) = task_keys.remove(&task_id) {
                tracing::debug!("Adding {} keys from aborted task to pending", keys.len());
                results.add_pending_keys(keys);
            }
        }

        if let Some((extra, headers)) = cells.cell_list.iter().find_map(|name| {
            extra_by_cell
                .remove(name)
                .map(|e| (e, headers_by_cell.remove(name).unwrap_or_default()))
        }) {
            results.extra_fields = extra;
            results.http_headers = headers;
        }

        results
    }

    /// Handles the completion of a single upstream task.
    ///
    /// Processes task join results, handles failures, and extracts response metadata.
    /// Returns cell name, extra fields, headers, and success status for successful requests.
    /// Failed tasks have their keys added to pending.
    fn handle_task_completion(
        &self,
        join_result: Result<(tokio::task::Id, UpstreamTaskResult), tokio::task::JoinError>,
        task_keys: &mut HashMap<tokio::task::Id, Vec<String>>,
        results: &mut MergedResults,
    ) -> Option<(
        String,
        HashMap<String, serde_json::Value>,
        hyper::header::HeaderMap,
        bool,
    )> {
        let (task_id, upstream_result) = match join_result {
            Ok((id, result)) => (id, result),
            Err(e) => {
                tracing::error!("Task failed: {e}");
                if let Some(keys) = task_keys.remove(&e.id()) {
                    results.add_pending_keys(keys);
                }
                return None;
            }
        };

        task_keys.remove(&task_id);

        let request_succeeded = upstream_result.result.is_ok();
        let (cell_name, extra, headers) = self.process_result(upstream_result, results)?;
        Some((cell_name, extra, headers, request_succeeded))
    }

    fn process_result(
        &self,
        upstream_result: UpstreamTaskResult,
        results: &mut MergedResults,
    ) -> Option<(
        String,
        HashMap<String, serde_json::Value>,
        hyper::header::HeaderMap,
    )> {
        let cell_name = upstream_result.cell_name;

        let Ok(response) = upstream_result.result else {
            results.add_pending_keys(upstream_result.public_keys);
            return None;
        };

        let (parts, body) = response.into_parts();

        match ProjectConfigsResponse::from_bytes(&body) {
            Ok(data) => {
                results.merge_project_configs(data.project_configs);
                if let Some(pending) = data.pending_keys {
                    results.add_pending_keys(pending);
                }
                Some((cell_name, data.extra_fields, parts.headers))
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to parse response");
                results.add_pending_keys(upstream_result.public_keys);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;
    use crate::locale::Locales;
    use hyper::Method;
    use hyper::service::service_fn;
    use hyper_util::rt::TokioExecutor;
    use std::convert::Infallible;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;
    use url::Url;

    /// Start a mock HTTP server that responds with custom data
    async fn start_mock_server<F>(response_fn: F) -> u16
    where
        F: Fn() -> ProjectConfigsResponse + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let response_fn = Arc::new(Mutex::new(response_fn));

        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let io = hyper_util::rt::TokioIo::new(stream);
                let response_fn = response_fn.clone();

                tokio::spawn(async move {
                    let service = service_fn(move |_req: Request<hyper::body::Incoming>| {
                        let response_fn = response_fn.clone();
                        async move {
                            let response = (response_fn.lock().await)();
                            let json = serde_json::to_vec(&response).unwrap();
                            Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(json))))
                        }
                    });

                    let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                        .serve_connection(io, service)
                        .await;
                });
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        port
    }

    fn test_cells(ports: Vec<(&str, u16)>) -> Cells {
        let cell_configs: Vec<CellConfig> = ports
            .into_iter()
            .map(|(name, port)| CellConfig {
                name: name.to_string(),
                sentry_url: Url::parse(&format!("http://127.0.0.1:{}", port)).unwrap(),
                relay_url: Url::parse(&format!("http://127.0.0.1:{}", port)).unwrap(),
            })
            .collect();

        let mut locales_map = HashMap::new();
        locales_map.insert("test".to_string(), cell_configs);
        Locales::new(locales_map).get_cells("test").unwrap().clone()
    }

    fn test_executor() -> UpstreamTaskExecutor {
        let connector = HttpConnector::new();
        let client = Client::builder(TokioExecutor::new()).build(connector);
        let timeouts = RelayTimeouts {
            http_timeout_secs: 5,
            task_initial_timeout_secs: 10,
            task_subsequent_timeout_secs: 2,
        };
        UpstreamTaskExecutor::new(client, timeouts)
    }

    fn test_split_request(cells: &Cells, cell_name: &str, keys: Vec<String>) -> SplitRequest {
        let upstream = cells.cell_to_upstreams.get(cell_name).unwrap().clone();
        SplitRequest {
            cell_name: cell_name.to_string(),
            upstream,
            request: ProjectConfigsRequest {
                public_keys: keys,
                extra_fields: HashMap::new(),
            },
        }
    }

    #[tokio::test]
    async fn test_multiple_upstreams_all_succeed() {
        // Mock server 1 returns config for key1
        let port1 = start_mock_server(|| {
            let mut resp = ProjectConfigsResponse {
                project_configs: HashMap::new(),
                pending_keys: None,
                extra_fields: HashMap::new(),
            };
            resp.project_configs.insert(
                "key1".to_string(),
                serde_json::json!({"disabled": false, "slug": "project1"}),
            );
            resp.extra_fields
                .insert("global".to_string(), serde_json::json!({"from": "cell1"}));
            resp
        })
        .await;

        // Mock server 2 returns config for key2
        let port2 = start_mock_server(|| {
            let mut resp = ProjectConfigsResponse {
                project_configs: HashMap::new(),
                pending_keys: None,
                extra_fields: HashMap::new(),
            };
            resp.project_configs.insert(
                "key2".to_string(),
                serde_json::json!({"disabled": false, "slug": "project2"}),
            );
            resp.extra_fields
                .insert("global".to_string(), serde_json::json!({"from": "cell2"}));
            resp
        })
        .await;

        let cells = test_cells(vec![("cell1", port1), ("cell2", port2)]);
        let executor = test_executor();

        let split_requests = vec![
            test_split_request(&cells, "cell1", vec!["key1".to_string()]),
            test_split_request(&cells, "cell2", vec!["key2".to_string()]),
        ];

        let base_request = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .body(())
            .unwrap();

        let results = executor
            .execute(split_requests, &base_request, &cells)
            .await
            .unwrap();

        // Both configs should be merged
        assert_eq!(results.project_configs.len(), 2);
        assert!(results.project_configs.contains_key("key1"));
        assert!(results.project_configs.contains_key("key2"));

        // Should select extra fields from cell1 (highest priority)
        assert_eq!(
            results.extra_fields.get("global"),
            Some(&serde_json::json!({"from": "cell1"}))
        );

        assert!(results.pending_keys.is_empty());
    }

    #[tokio::test]
    async fn test_partial_failure() {
        // Mock server 1 succeeds
        let port1 = start_mock_server(|| {
            let mut resp = ProjectConfigsResponse {
                project_configs: HashMap::new(),
                pending_keys: None,
                extra_fields: HashMap::new(),
            };
            resp.project_configs
                .insert("key1".to_string(), serde_json::json!({"disabled": false}));
            resp
        })
        .await;

        // cell2 will fail (invalid port)
        let cells = test_cells(vec![("cell1", port1), ("cell2", 1)]);
        let executor = test_executor();

        let split_requests = vec![
            test_split_request(&cells, "cell1", vec!["key1".to_string()]),
            test_split_request(
                &cells,
                "cell2",
                vec!["key2".to_string(), "key3".to_string()],
            ),
        ];

        let base_request = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .body(())
            .unwrap();

        let results = executor
            .execute(split_requests, &base_request, &cells)
            .await
            .unwrap();

        // Successful config from cell1
        assert_eq!(results.project_configs.len(), 1);
        assert!(results.project_configs.contains_key("key1"));

        // Failed keys from cell2 should be in pending
        assert_eq!(results.pending_keys.len(), 2);
        assert!(results.pending_keys.contains(&"key2".to_string()));
        assert!(results.pending_keys.contains(&"key3".to_string()));
    }

    #[tokio::test]
    async fn test_upstream_returns_pending_keys() {
        let port = start_mock_server(|| {
            let mut resp = ProjectConfigsResponse {
                project_configs: HashMap::new(),
                pending_keys: None,
                extra_fields: HashMap::new(),
            };
            resp.project_configs
                .insert("key1".to_string(), serde_json::json!({"disabled": false}));
            // Upstream says key2 is pending
            resp.pending_keys = Some(vec!["key2".to_string()]);
            resp
        })
        .await;

        let cells = test_cells(vec![("cell1", port)]);
        let executor = test_executor();

        let split_requests = vec![test_split_request(
            &cells,
            "cell1",
            vec!["key1".to_string(), "key2".to_string()],
        )];

        let base_request = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .body(())
            .unwrap();

        let results = executor
            .execute(split_requests, &base_request, &cells)
            .await
            .unwrap();

        // key1 in configs
        assert!(results.project_configs.contains_key("key1"));

        // key2 in pending (from upstream)
        assert_eq!(results.pending_keys.len(), 1);
        assert!(results.pending_keys.contains(&"key2".to_string()));
    }

    #[tokio::test]
    async fn test_empty_split_requests() {
        let cells = test_cells(vec![("cell1", 8080)]);
        let executor = test_executor();

        let split_requests = vec![];
        let base_request = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .body(())
            .unwrap();

        let results = executor
            .execute(split_requests, &base_request, &cells)
            .await
            .unwrap();

        assert!(results.project_configs.is_empty());
        assert!(results.pending_keys.is_empty());
        assert!(results.extra_fields.is_empty());
    }
}
