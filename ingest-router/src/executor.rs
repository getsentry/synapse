//! Task execution for parallel upstream requests.

use crate::config::RelayTimeouts;
use crate::errors::{IngestRouterError, Result};
use crate::handler::{CellId, UpstreamRequest, UpstreamResponse};
use crate::http::send_to_upstream;
use http_body_util::Full;
use hyper::Request;
use hyper::body::Bytes;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::Instant;
use url::Url;

/// Typed error for failed upstream requests that preserves the original request.
#[derive(Debug)]
pub struct RequestFailed<Req> {
    /// The original typed request that failed
    pub request: Req,
}

impl<Req: Serialize> From<RequestFailed<Req>> for IngestRouterError {
    fn from(error: RequestFailed<Req>) -> Self {
        IngestRouterError::RequestFailedWithData {
            request_json: serde_json::to_value(error.request)
                .unwrap_or(serde_json::json!({"error": "failed to serialize request"})),
        }
    }
}

/// Result of a single upstream task execution
type TaskResult<Req, Res> = Result<(CellId, Res), (CellId, RequestFailed<Req>)>;

/// JoinSet for upstream task execution
type TaskJoinSet<Req, Res> = JoinSet<(CellId, Result<Res, RequestFailed<Req>>)>;

/// Map tracking typed request data for in-flight tasks
type InFlightRequests<Req> = HashMap<CellId, Arc<Req>>;

/// State machine for adaptive timeout collection strategy.
enum CollectionState {
    /// Initial phase: waiting for first successful response.
    WaitingForFirst,

    /// Subsequent phase: first success received, collecting remaining.
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
/// This executor fans out requests to multiple upstream instances simultaneously,
/// collects their responses, and returns them. It implements a two-phase adaptive timeout
/// strategy to balance responsiveness with resilience:
///
/// 1. **Initial phase**: Wait up to `task_initial_timeout_secs` for the first upstream to respond
/// 2. **Subsequent phase**: Once first success occurs, give all remaining tasks `task_subsequent_timeout_secs` total to complete
///
/// This ensures fast cells aren't blocked by slow/failing cells, while still allowing
/// sufficient time for healthy upstreams to respond.
#[allow(dead_code)]
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

    /// Validates HTTP response status and extracts body.
    fn validate_response(
        response: hyper::Response<Bytes>,
    ) -> Result<Bytes, (hyper::StatusCode, String)> {
        let status = response.status();

        if !status.is_success() {
            let body = response.into_body();
            let error_body = String::from_utf8_lossy(&body).into_owned();
            tracing::error!(
                status = %status,
                body = %error_body,
                "Upstream returned non-success status"
            );
            return Err((status, error_body));
        }

        Ok(response.into_body())
    }

    /// Processes the upstream response: validates status and parses body.
    fn parse_upstream_response<Req, Res>(
        result: Result<hyper::Response<Bytes>, IngestRouterError>,
        request: Arc<Req>,
    ) -> Result<Res, RequestFailed<Req>>
    where
        Req: Clone,
        Res: DeserializeOwned,
    {
        match result {
            Ok(response) => {
                // Validate HTTP status and extract body
                let body =
                    Self::validate_response(response).map_err(|(_status, _error_body)| {
                        RequestFailed {
                            request: Arc::try_unwrap(request.clone())
                                .unwrap_or_else(|arc| (*arc).clone()),
                        }
                    })?;

                // Parse response body
                serde_json::from_slice::<Res>(&body).map_err(|e| {
                    tracing::error!(
                        error = %e,
                        "Failed to parse upstream response"
                    );

                    RequestFailed {
                        request: Arc::try_unwrap(request).unwrap_or_else(|arc| (*arc).clone()),
                    }
                })
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "Request to upstream failed"
                );

                Err(RequestFailed {
                    request: Arc::try_unwrap(request).unwrap_or_else(|arc| (*arc).clone()),
                })
            }
        }
    }

    /// Spawns and collects results from all upstream tasks.
    pub async fn execute<Req, Res>(
        &self,
        cell_requests: Vec<(CellId, Url, Req)>,
        base_request: &Request<()>,
    ) -> Vec<TaskResult<Req, Res>>
    where
        Req: UpstreamRequest,
        Res: UpstreamResponse,
    {
        let (mut join_set, mut in_flight_requests) =
            self.spawn_upstream_tasks(cell_requests, base_request);
        self.collect_results(&mut join_set, &mut in_flight_requests)
            .await
    }

    /// Spawns parallel tasks for all upstream requests.
    ///
    /// Returns a JoinSet of spawned tasks and a map tracking in-flight requests
    fn spawn_upstream_tasks<Req, Res>(
        &self,
        cell_requests: Vec<(CellId, Url, Req)>,
        base_request: &Request<()>,
    ) -> (TaskJoinSet<Req, Res>, InFlightRequests<Req>)
    where
        Req: UpstreamRequest,
        Res: UpstreamResponse,
    {
        let mut join_set = JoinSet::new();
        let mut in_flight_requests = HashMap::new();

        for (cell_id, upstream_url, req) in cell_requests {
            let req_arc = Arc::new(req);

            let request = match self.build_upstream_request(&*req_arc, base_request) {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(
                        cell_id = %cell_id,
                        error = %e,
                        "Failed to build upstream request"
                    );

                    let req_owned = Arc::try_unwrap(req_arc).unwrap_or_else(|arc| (*arc).clone());

                    join_set
                        .spawn(async move { (cell_id, Err(RequestFailed { request: req_owned })) });
                    continue;
                }
            };

            in_flight_requests.insert(cell_id.clone(), Arc::clone(&req_arc));

            let client = self.client.clone();
            let http_timeout = self.timeouts.http_timeout_secs as u64;

            join_set.spawn(async move {
                let result = send_to_upstream(&client, &upstream_url, request, http_timeout).await;
                let parsed_result = Self::parse_upstream_response(result, req_arc);
                (cell_id, parsed_result)
            });
        }

        (join_set, in_flight_requests)
    }

    /// Builds an HTTP request to send to an upstream Sentry instance.
    ///
    /// Copies method, URI, version, and headers from the original request,
    /// but replaces the body with the serialized request data.
    fn build_upstream_request<Req>(
        &self,
        req: &Req,
        base_request: &Request<()>,
    ) -> Result<Request<Full<Bytes>>>
    where
        Req: Serialize,
    {
        let request_body = serde_json::to_vec(req).map_err(|e| {
            IngestRouterError::InternalError(format!("Failed to serialize request: {e}"))
        })?;

        let mut req_builder = Request::builder()
            .method(base_request.method())
            .uri(base_request.uri())
            .version(base_request.version());

        for (name, value) in base_request.headers() {
            req_builder = req_builder.header(name, value);
        }

        req_builder
            .body(Full::new(Bytes::from(request_body)))
            .map_err(|e| {
                IngestRouterError::InternalError(format!("Failed to build HTTP request: {e}"))
            })
    }

    /// Drains any remaining tasks from the join set after abort.
    async fn drain_remaining_tasks<Req: 'static, Res: Send + 'static>(
        join_set: &mut TaskJoinSet<Req, Res>,
        in_flight_requests: &mut InFlightRequests<Req>,
        results: &mut Vec<TaskResult<Req, Res>>,
    ) {
        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((cell_id, result)) => {
                    in_flight_requests.remove(&cell_id);
                    match result {
                        Ok(res) => results.push(Ok((cell_id, res))),
                        Err(e) => results.push(Err((cell_id, e))),
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "Task failed");
                }
            }
        }
    }

    /// Collects results with adaptive timeout strategy using state machine.
    ///
    /// The `in_flight_requests` map tracks which requests are still in-flight. When tasks
    /// complete (successfully or with errors), they are removed from this map. When tasks
    /// are aborted due to timeout, the remaining entries in this map are used to create
    /// typed error results that preserve the original request data (no serialization).
    async fn collect_results<Req, Res>(
        &self,
        join_set: &mut TaskJoinSet<Req, Res>,
        in_flight_requests: &mut InFlightRequests<Req>,
    ) -> Vec<TaskResult<Req, Res>>
    where
        Req: Clone + Send + Sync + 'static,
        Res: Send + 'static,
    {
        let mut results = Vec::new();
        let mut state = CollectionState::WaitingForFirst;
        let deadline = tokio::time::sleep_until(state.current_deadline(&self.timeouts));
        tokio::pin!(deadline);

        // Main collection loop with adaptive timeout
        loop {
            tokio::select! {
                Some(join_result) = join_set.join_next() => {
                    match join_result {
                        Ok((cell_id, result)) => {
                            // Handle successful or failed task result
                            let is_success = result.is_ok();
                            in_flight_requests.remove(&cell_id);
                            match result {
                                Ok(res) => results.push(Ok((cell_id, res))),
                                Err(e) => results.push(Err((cell_id, e))),
                            }

                            // Transition to subsequent phase on first success
                            if is_success && matches!(state, CollectionState::WaitingForFirst) {
                                state.transition_to_subsequent();
                                deadline.as_mut().reset(state.current_deadline(&self.timeouts));
                            }
                        }
                        Err(e) => {
                            // Task panicked or was unexpectedly cancelled
                            // We cannot extract cell_id from JoinError, so the request
                            // will be recovered from in_flight_requests at the end
                            // and converted to an error result
                            tracing::error!(error = %e, "Task failed to complete");
                        }
                    }
                }
                _ = &mut deadline => {
                    tracing::error!("Deadline reached. Aborting tasks");
                    join_set.abort_all();
                    break;
                }
                else => {
                    break;
                }
            }
        }

        // Drain any remaining aborted tasks
        Self::drain_remaining_tasks(join_set, in_flight_requests, &mut results).await;

        // Convert remaining in-flight requests to timeout errors
        for (cell_id, request) in in_flight_requests.drain() {
            let request = Arc::try_unwrap(request).unwrap_or_else(|arc| (*arc).clone());
            results.push(Err((cell_id, RequestFailed { request })));
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;
    use crate::locale::{Cells, Locales};
    use crate::project_config::protocol::{ProjectConfigsRequest, ProjectConfigsResponse};
    use hyper::body::Incoming;
    use hyper::service::service_fn;
    use hyper::{Method, Response};
    use hyper_util::rt::TokioExecutor;
    use std::collections::HashMap;
    use std::convert::Infallible;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;
    use url::Url;

    // Test configuration constants
    const TEST_SERVER_STARTUP_DELAY_MS: u64 = 50;
    const TEST_HTTP_TIMEOUT_SECS: u16 = 5;
    const TEST_INITIAL_TIMEOUT_SECS: u16 = 10;
    const TEST_SUBSEQUENT_TIMEOUT_SECS: u16 = 2;

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
                    let service = service_fn(move |_req: Request<Incoming>| {
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

        tokio::time::sleep(Duration::from_millis(TEST_SERVER_STARTUP_DELAY_MS)).await;
        port
    }

    /// Helper to create a test response with project configs and pending keys
    fn create_test_response(
        configs: Vec<(&str, serde_json::Value)>,
        pending: Vec<&str>,
        extra: Option<(&str, serde_json::Value)>,
    ) -> ProjectConfigsResponse {
        let mut resp = ProjectConfigsResponse {
            project_configs: configs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            pending_keys: pending.into_iter().map(|s| s.to_string()).collect(),
            extra_fields: HashMap::new(),
            http_headers: hyper::HeaderMap::new(),
        };
        if let Some((key, value)) = extra {
            resp.extra_fields.insert(key.to_string(), value);
        }
        resp
    }

    fn test_cells(ports: Vec<(&str, u16)>) -> Cells {
        let cell_configs: Vec<CellConfig> = ports
            .into_iter()
            .map(|(id, port)| CellConfig {
                id: id.to_string(),
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
            http_timeout_secs: TEST_HTTP_TIMEOUT_SECS,
            task_initial_timeout_secs: TEST_INITIAL_TIMEOUT_SECS,
            task_subsequent_timeout_secs: TEST_SUBSEQUENT_TIMEOUT_SECS,
        };
        UpstreamTaskExecutor::new(client, timeouts)
    }

    fn test_cell_request(
        cells: &Cells,
        cell_id: &str,
        keys: Vec<String>,
    ) -> (CellId, Url, ProjectConfigsRequest) {
        let upstream = cells.cell_to_upstreams.get(cell_id).unwrap();
        (
            cell_id.to_string(),
            upstream.sentry_url.clone(),
            ProjectConfigsRequest {
                public_keys: keys,
                extra_fields: HashMap::new(),
            },
        )
    }

    #[tokio::test]
    async fn test_multiple_upstreams_all_succeed() {
        // Mock server 1 returns config for key1
        let port1 = start_mock_server(|| {
            create_test_response(
                vec![(
                    "key1",
                    serde_json::json!({"disabled": false, "slug": "project1"}),
                )],
                vec![],
                Some(("global", serde_json::json!({"from": "cell1"}))),
            )
        })
        .await;

        // Mock server 2 returns config for key2
        let port2 = start_mock_server(|| {
            create_test_response(
                vec![(
                    "key2",
                    serde_json::json!({"disabled": false, "slug": "project2"}),
                )],
                vec![],
                Some(("global", serde_json::json!({"from": "cell2"}))),
            )
        })
        .await;

        let cells = test_cells(vec![("cell1", port1), ("cell2", port2)]);
        let executor = test_executor();

        let cell_requests = vec![
            test_cell_request(&cells, "cell1", vec!["key1".to_string()]),
            test_cell_request(&cells, "cell2", vec!["key2".to_string()]),
        ];

        let base_request = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .body(())
            .unwrap();

        let results = executor
            .execute::<ProjectConfigsRequest, ProjectConfigsResponse>(cell_requests, &base_request)
            .await;

        // Both cells should succeed
        assert_eq!(results.len(), 2);
        let success_count = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(success_count, 2);

        // Check that cell1 succeeded with key1
        let cell1_result = results.iter().find(|r| {
            r.as_ref()
                .ok()
                .map(|(id, _)| id == "cell1")
                .unwrap_or(false)
        });
        assert!(cell1_result.is_some());
        if let Some(Ok((_, response))) = cell1_result {
            assert!(response.project_configs.contains_key("key1"));
        }

        // Check that cell2 succeeded with key2
        let cell2_result = results.iter().find(|r| {
            r.as_ref()
                .ok()
                .map(|(id, _)| id == "cell2")
                .unwrap_or(false)
        });
        assert!(cell2_result.is_some());
        if let Some(Ok((_, response))) = cell2_result {
            assert!(response.project_configs.contains_key("key2"));
        }
    }

    #[tokio::test]
    async fn test_upstream_failure_scenarios() {
        // Scenario 1: Partial failure (1 success, 1 failure)
        {
            // Mock server 1 succeeds
            let port1 = start_mock_server(|| {
                create_test_response(
                    vec![("key1", serde_json::json!({"disabled": false}))],
                    vec![],
                    None,
                )
            })
            .await;

            // cell2 will fail (invalid port)
            let cells = test_cells(vec![("cell1", port1), ("cell2", 1)]);
            let executor = test_executor();

            let cell_requests = vec![
                test_cell_request(&cells, "cell1", vec!["key1".to_string()]),
                test_cell_request(
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
                .execute::<ProjectConfigsRequest, ProjectConfigsResponse>(
                    cell_requests,
                    &base_request,
                )
                .await;

            // Should have 2 results: 1 success, 1 failure
            assert_eq!(results.len(), 2);

            // cell1 should succeed
            let cell1_result = results.iter().find(|r| match r {
                Ok((id, _)) => id == "cell1",
                Err((id, _)) => id == "cell1",
            });
            assert!(cell1_result.is_some());
            assert!(cell1_result.unwrap().is_ok());

            // cell2 should fail
            let cell2_result = results.iter().find(|r| match r {
                Ok((id, _)) => id == "cell2",
                Err((id, _)) => id == "cell2",
            });
            assert!(cell2_result.is_some());
            assert!(cell2_result.unwrap().is_err());
        }

        // Scenario 2: All upstreams failing
        {
            let cells = test_cells(vec![
                ("cell1", 1), // Invalid port - connection will fail
                ("cell2", 2), // Invalid port - connection will fail
            ]);

            let executor = test_executor();

            let cell_requests = vec![
                test_cell_request(&cells, "cell1", vec!["key1".to_string()]),
                test_cell_request(&cells, "cell2", vec!["key2".to_string()]),
            ];

            let base_request = Request::builder()
                .method(Method::POST)
                .uri("/test")
                .body(())
                .unwrap();

            let results = executor
                .execute::<ProjectConfigsRequest, ProjectConfigsResponse>(
                    cell_requests,
                    &base_request,
                )
                .await;

            // Should have 2 results, both failures
            assert_eq!(results.len(), 2);
            let failure_count = results.iter().filter(|r| r.is_err()).count();
            assert_eq!(failure_count, 2, "All upstreams should fail");

            // Both should preserve typed request data
            for result in &results {
                assert!(result.is_err());
                if let Err((_, error)) = result {
                    // Should have the original typed request data
                    assert!(!error.request.public_keys.is_empty());
                }
            }
        }
    }

    #[tokio::test]
    async fn test_empty_cell_requests() {
        let executor = test_executor();

        let cell_requests = vec![];
        let base_request = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .body(())
            .unwrap();

        let results = executor
            .execute::<ProjectConfigsRequest, ProjectConfigsResponse>(cell_requests, &base_request)
            .await;

        // Should have no results
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_initial_timeout_all_slow() {
        let cells = test_cells(vec![
            ("cell1", 1), // Invalid port - will fail to connect, simulating hung upstream
        ]);

        // Use very short timeouts for this test
        let connector = HttpConnector::new();
        let client = Client::builder(TokioExecutor::new()).build(connector);
        let timeouts = RelayTimeouts {
            http_timeout_secs: 1,
            task_initial_timeout_secs: 1, // Short initial timeout
            task_subsequent_timeout_secs: 1,
        };
        let executor = UpstreamTaskExecutor::new(client, timeouts);

        let cell_requests = vec![test_cell_request(&cells, "cell1", vec!["key1".to_string()])];

        let base_request = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .body(())
            .unwrap();

        let start = std::time::Instant::now();
        let results = executor
            .execute::<ProjectConfigsRequest, ProjectConfigsResponse>(cell_requests, &base_request)
            .await;
        let elapsed = start.elapsed();

        // Should timeout and return error
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());

        // Verify it timed out reasonably quickly (allowing some margin for processing)
        assert!(
            elapsed < Duration::from_secs(3),
            "Should timeout quickly, took {:?}",
            elapsed
        );

        if let Err((_, error)) = &results[0] {
            // Should be RequestFailed with preserved typed request data
            assert!(
                !error.request.public_keys.is_empty(),
                "Request data should be preserved"
            );
        }
    }

    #[tokio::test]
    async fn test_subsequent_timeout_after_first_success() {
        // Fast server responds quickly
        let port1 = start_mock_server(|| {
            create_test_response(
                vec![("key1", serde_json::json!({"disabled": false}))],
                vec![],
                None,
            )
        })
        .await;

        // Slow cell - use invalid port to simulate hung connection
        let cells = test_cells(vec![("cell1", port1), ("cell2", 2)]);

        // Use shorter timeouts to make test faster
        let connector = HttpConnector::new();
        let client = Client::builder(TokioExecutor::new()).build(connector);
        let timeouts = RelayTimeouts {
            http_timeout_secs: 5,
            task_initial_timeout_secs: 10,
            task_subsequent_timeout_secs: 1, // Very short subsequent timeout
        };
        let executor = UpstreamTaskExecutor::new(client, timeouts);

        let cell_requests = vec![
            test_cell_request(&cells, "cell1", vec!["key1".to_string()]),
            test_cell_request(&cells, "cell2", vec!["key2".to_string()]),
        ];

        let base_request = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .body(())
            .unwrap();

        let start = std::time::Instant::now();
        let results = executor
            .execute::<ProjectConfigsRequest, ProjectConfigsResponse>(cell_requests, &base_request)
            .await;
        let elapsed = start.elapsed();

        // Should have 2 results: cell1 success, cell2 failure
        assert_eq!(results.len(), 2);

        // cell1 should succeed
        let cell1_result = results.iter().find(|r| match r {
            Ok((id, _)) => id == "cell1",
            Err((id, _)) => id == "cell1",
        });
        assert!(cell1_result.is_some());
        assert!(cell1_result.unwrap().is_ok());

        // cell2 should fail (either timeout or connection error)
        let cell2_result = results.iter().find(|r| match r {
            Ok((id, _)) => id == "cell2",
            Err((id, _)) => id == "cell2",
        });
        assert!(cell2_result.is_some());
        assert!(cell2_result.unwrap().is_err());

        // Should timeout quickly after first success (not waiting for full HTTP timeout)
        // The subsequent timeout is 1s, so should complete well before the HTTP timeout of 5s
        assert!(
            elapsed < Duration::from_secs(5),
            "Should timeout quickly after first success, took {:?}",
            elapsed
        );
    }
}
