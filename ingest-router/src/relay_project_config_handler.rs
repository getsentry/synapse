//! Relay Project Configuration Handler
//!
//! This module implements the split-merge strategy for handling Sentry's
//! `/api/0/relays/projectconfigs/` endpoint across multiple upstream Sentry instances
//! in a multi-cell architecture.
//!
//! # Protocol Overview
//!
//! The Relay Project Configs endpoint (version 3) is used by Relay instances to fetch
//! project configurations needed to process events. This implementation acts as an
//! aggregation layer that:
//! 1. Splits requests across multiple upstream Sentry cells based on project ownership
//! 2. Fans out requests in parallel to multiple upstreams
//! 3. Merges responses back into a single v3-compliant response
//! 4. Passes through all config data unchanged
//!
//! ## Endpoint Details
//! The endpoint implementation is at https://github.com/getsentry/sentry/blob/master/src/sentry/api/endpoints/relay/project_configs.py
//!
//! - **Path**: `/api/0/relays/projectconfigs/`
//! - **Method**: `POST`
//! - **Protocol Version**: 3
//! - **Authentication**: RelayAuthentication (X-Sentry-Relay-Id, X-Sentry-Relay-Signature)
//!
//! # Request Format (Version 3)
//!
//! ```json
//! {
//!   "publicKeys": ["key1", "key2", "key3"],
//!   "noCache": false,
//!   "global": true
//! }
//! ```
//!
//! ## Request Fields
//!
//! - **`publicKeys`** (required): Array of project public keys (DSN keys) to fetch configs for
//! - **`noCache`** (optional): If `true`, bypass caching and compute fresh configs (downgrades to v2 behavior)
//! - **`global`** (optional): If `true`, include global relay configuration in response
//!
//! # Response Format (Version 3)
//!
//! ```json
//! {
//!   "configs": {
//!     "key1": {
//!       "disabled": false,
//!       "slug": "project-slug",
//!       "publicKeys": [...],
//!       "config": {...},
//!       "organizationId": 1,
//!       "projectId": 42
//!     }
//!   },
//!   "pending": ["key2", "key3"],
//!   "global": {
//!     "measurements": {...},
//!     "options": {...}
//!   },
//!   "global_status": "ready"
//! }
//! ```
//!
//! ## Response Fields
//!
//! - **`configs`**: Map of public keys to their project configurations
//!   - Configs are passed through unchanged from upstream Sentry instances
//!   - They will add the relevant processing relay URL in the response
//! - **`pending`**: Array of public keys for which configs are being computed asynchronously
//!   - Relay should retry the request later to fetch these configs
//!   - Also used when upstreams fail/timeout (graceful degradation)
//! - **`global`**: Global relay configuration (only if `global: true` in request)
//! - **`global_status`**: Status of global config (always "ready" when present)
//!
//! # Implementation Details
//!
//! ## Request Splitting Strategy
//!
//! When a request arrives with multiple public keys:
//!
//! 1. **Route each key to its owning cell**
//!    - Currently: Round-robin distribution (TODO: replace with control plane lookup)
//!    - TODO: Query locator service for each public key to get cell name
//!
//! 2. **Group keys by target upstream**
//!    - Keys routed to the same cell are batched into one request
//!
//! 3. **Handle global config flag**
//!    - All upstreams receive the same `global` flag value as the original request
//!    - Global config is selected from the highest priority cell that returns it
//!    - Priority is determined by cell order in configuration (first = highest priority)
//!    - Enables failover: if highest priority cell fails, next cell's global config is used
//!
//! ## Response Merging Strategy
//!
//! Responses from multiple upstreams are merged as follows:
//!
//! ### Configs (HashMap merge)
//! - Merge all `configs` HashMaps from all upstreams
//! - Configs are passed through unchanged (no modifications)
//! - relay_url is expected to be added in the upstream response
//!
//! ### Pending (Array concatenation)
//! - Concatenate all `pending` arrays from all upstream responses
//! - Include keys from failed/timed-out upstreams
//! - Relay will retry these keys in a subsequent request
//!
//! ### Extra fields (Priority-based selection)
//! - Select `extra` fields from highest priority cell that responds successfully
//! - Priority determined by cell order in configuration (first = highest)
//! - Forward compatibility: new fields are automatically preserved
//!
//! ## Error Handling
//!
//! ### Partial Failures (Graceful Degradation)
//! - If some upstreams succeed: Return successful configs + pending for failed keys
//! - Failed keys are added to `pending` array (v3 protocol)
//! - Logged but does not block response
//!
//! ### Total Failure
//! - If all upstreams fail: Check if any keys were added to pending
//! - If pending is not empty: Return 200 OK with pending array (relay will retry)
//! - If pending is empty: Return 503 error (no recoverable state)
//!
//! ### Upstream Failure Scenarios
//! - **Timeout**: All keys from that upstream → pending
//! - **Connection error**: All keys from that upstream → pending
//! - **Parse error**: All keys from that upstream → pending
//! - **Task panic**: Logged error (extreme edge case, keys lost)
//!
//! ## Request Flow
//!
//! ### Success Scenario
//!
//! ```text
//! ┌─────────────┐
//! │   Relay     │
//! └──────┬──────┘
//!        │
//!        │ POST /api/0/relays/projectconfigs/
//!        │ {publicKeys: [A,B,C,D,E,F]}
//!        │
//!        ▼
//! ┌──────────────────────────────────────┐
//! │      Ingest Router (this handler)    │
//! │                                      │
//! │  1. Parse request                    │
//! │  2. Split keys by cell:              │
//! │     • US1 → [A,C,E]                  │
//! │     • US2 → [B,D,F]                  │
//! └───┬──────────────────────────┬───────┘
//!     │                          │
//!     │ {publicKeys: [A,C,E],    │ {publicKeys: [B,D,F],
//!     │  global: true}           │  global: true}
//!     │                          │
//!     ▼                          ▼
//! ┌──────────┐              ┌──────────┐
//! │Cell US1  │              │Cell US2  │
//! │(Sentry)  │              │(Sentry)  │
//! └────┬─────┘              └─────┬────┘
//!      │                          │
//!      │ {configs: {A,C,E}}       │ {configs: {B,D,F}}
//!      │                          │
//!      └──────────┬───────────────┘
//!                 ▼
//! ┌──────────────────────────────────────┐
//! │      Ingest Router (this handler)    │
//! │                                      │
//! │  3. Merge responses:                 │
//! │     • Combine all configs            │
//! │     • Merge pending arrays           │
//! │     • Select others from priority    │
//! └──────────────┬───────────────────────┘
//!                │
//!                │ {configs: {A,B,C,D,E,F},
//!                │  global: {...}}
//!                │
//!                ▼
//!         ┌─────────────┐
//!         │   Relay     │
//!         └─────────────┘
//! ```
//!
//! ### Failure Scenario (Graceful Degradation)
//!
//! When an upstream fails or times out, keys are added to the `pending` array:
//!
//! ```text
//! ┌─────────────┐
//! │   Relay     │
//! └──────┬──────┘
//!        │
//!        │ POST {publicKeys: [A,B,C,D,E,F]}
//!        │
//!        ▼
//! ┌──────────────────────────────────────┐
//! │      Ingest Router (this handler)    │
//! │  Split: US1→[A,C,E], US2→[B,D,F]     │
//! └───┬──────────────────────────┬───────┘
//!     │                          │
//!     ▼                          ▼
//! ┌──────────┐              ┌──────────┐
//! │Cell US1  │              │Cell US2  │
//! └────┬─────┘              └─────┬────┘
//!      │                          │
//!      │ ✓ Success                │ ✗ Timeout/Error
//!      │ {configs: {A,C,E}}       │
//!      │                          │
//!      └──────────┬───────────────┘
//!                 ▼
//! ┌──────────────────────────────────────┐
//! │      Ingest Router (this handler)    │
//! │                                      │
//! │  • Collect successful: {A,C,E}       │
//! │  • Add failed to pending: [B,D,F]    │
//! └──────────────┬───────────────────────┘
//!                │
//!                │ {configs: {A,C,E},
//!                │  pending: [B,D,F]}
//!                │
//!                ▼
//!         ┌─────────────┐
//!         │   Relay     │ (will retry pending)
//!         └─────────────┘
//! ```
//!
//! # Examples
//!
//! ## Example 1: All upstreams succeed
//!
//! **Request**:
//! ```json
//! {"publicKeys": ["key1", "key2"]}
//! ```
//!
//! **Response**:
//! ```json
//! {
//!   "configs": {
//!     "key1": {"disabled": false, "slug": "project-us1", ...},
//!     "key2": {"disabled": false, "slug": "project-us2", ...}
//!   }
//! }
//! ```
//!
//! ## Example 2: One upstream fails
//!
//! **Request**:
//! ```json
//! {"publicKeys": ["key1", "key2", "key3"]}
//! ```
//!
//! **Response** (if upstream with key2,key3 times out):
//! ```json
//! {
//!   "configs": {
//!     "key1": {"disabled": false, "slug": "project-us1", ...}
//!   },
//!   "pending": ["key2", "key3"]
//! }
//! ```
//!
//! ## Example 3: Request with global config
//!
//! **Request**:
//! ```json
//! {"publicKeys": ["key1", "key2"], "global": true}
//! ```
//!
//! **Splitting**:
//! - Request to US1: `{"publicKeys": ["key1"], "global": true}`
//! - Request to US2: `{"publicKeys": ["key2"], "global": true}`
//!
//! (US1 has higher priority, so its global config will be used in the final response)
//!
//! **Response**:
//! ```json
//! {
//!   "configs": {...},
//!   "global": {"measurements": {...}},
//!   "global_status": "ready"
//! }
//! ```
//!
//! # See Also
//!
//! - [`RelayProjectConfigsHandler`] - Main handler struct for processing requests

use crate::config::{CellConfig, RelayTimeouts};
use crate::errors::IngestRouterError;
use crate::http::send_to_upstream;
use crate::locale::{Cells, Locales, Upstream};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::{Body, Bytes};
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderMap};
use hyper::{Request, Response, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout};

/// Request format for the relay project configs endpoint.
///
/// See module-level docs for full protocol details.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RelayProjectConfigsRequest {
    /// DSN public keys to fetch configs for.
    #[serde(rename = "publicKeys")]
    pub public_keys: Vec<String>,

    /// Other fields (`global`, `noCache`, future fields) for forward compatibility.
    /// All fields are passed through as-is to upstreams.
    #[serde(flatten)]
    pub extra: HashMap<String, JsonValue>,
}

impl RelayProjectConfigsRequest {
    fn from_bytes(bytes: &Bytes) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    fn to_bytes(&self) -> Result<Bytes, serde_json::Error> {
        let json = serde_json::to_vec(self)?;
        Ok(Bytes::from(json))
    }
}

/// Response format for the relay project configs endpoint.
///
/// See module-level docs for merge strategy and field details.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RelayProjectConfigsResponse {
    /// Project configs (HashMap merged from all upstreams).
    pub configs: HashMap<String, JsonValue>,

    /// Keys being computed async or from failed upstreams (concatenated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<Vec<String>>,

    /// Other fields (`global`, `global_status`, future fields).
    #[serde(flatten)]
    pub extra: HashMap<String, JsonValue>,
}

impl RelayProjectConfigsResponse {
    fn from_bytes(bytes: &Bytes) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// Result from an upstream task request.
struct UpstreamTaskResult {
    /// The cell name this result is from.
    cell_name: String,
    /// The upstream URL that was contacted.
    sentry_url: String,
    /// The public keys that were requested from this upstream.
    public_keys: Vec<String>,
    /// The result of the upstream request.
    result: Result<Response<Bytes>, IngestRouterError>,
}

/// Merged results from all upstream tasks.
struct MergedResults {
    /// All configs merged from successful upstreams.
    configs: HashMap<String, JsonValue>,
    /// All pending keys (from failed upstreams or upstream pending arrays).
    pending: Vec<String>,
    /// Extra fields (global config, status, etc.).
    extra: HashMap<String, JsonValue>,
    /// Headers from upstream
    headers: HeaderMap,
}

/// Task handle paired with its public keys for graceful failure handling.
///
/// The public keys are tracked outside the task so they can be added to the
/// pending array if the task times out or panics, maintaining v3 protocol compliance.
struct TaskWithKeys {
    /// The spawned task handle.
    handle: JoinHandle<UpstreamTaskResult>,
    /// Public keys requested from this upstream.
    public_keys: Vec<String>,
}

/// A request split for a specific upstream cell.
struct SplitRequest {
    /// The cell name this request is for
    cell_name: String,
    /// The upstream to send the request to
    upstream: Upstream,
    /// The request body to send
    request: RelayProjectConfigsRequest,
}

/// Executor for spawning and collecting upstream tasks.
///
/// Encapsulates the parallel task execution logic for fanning out requests
/// to multiple upstream Sentry instances and collecting/merging their results.
struct UpstreamTaskExecutor {
    /// HTTP client for sending requests to upstream Sentry instances.
    client: Client<HttpConnector, Full<Bytes>>,

    /// Timeout configuration for HTTP and task-level timeouts.
    timeouts: RelayTimeouts,
}

impl UpstreamTaskExecutor {
    fn new(client: Client<HttpConnector, Full<Bytes>>, timeouts: RelayTimeouts) -> Self {
        Self { client, timeouts }
    }

    /// Spawns parallel tasks to fan out requests to multiple upstreams.
    ///
    /// Each task performs an HTTP request to an upstream Sentry instance with
    /// the appropriate subset of public keys.
    ///
    /// Uses two-layer timeout strategy:
    /// - HTTP timeout: Applied to individual HTTP requests
    /// - Task timeout: Applied at collection level with adaptive strategy
    fn spawn_tasks(
        &self,
        split_requests: Vec<SplitRequest>,
        base_request: &Request<()>,
    ) -> Result<Vec<TaskWithKeys>, IngestRouterError> {
        let mut tasks = Vec::new();

        for split in split_requests {
            let cell_name = split.cell_name;
            let upstream = split.upstream;
            let split_request = split.request;
            // Track public keys for this upstream
            let public_keys = split_request.public_keys.clone();

            // Serialize the split request body
            let request_body = split_request.to_bytes().map_err(|e| {
                IngestRouterError::InternalError(format!("Failed to serialize request: {e}"))
            })?;

            // Build request with headers from original request
            let mut req_builder = Request::builder()
                .method(base_request.method())
                .uri(base_request.uri().clone())
                .version(base_request.version());

            for (name, value) in base_request.headers() {
                req_builder = req_builder.header(name, value);
            }

            let request = req_builder
                .body(Full::new(request_body))
                .expect("Failed to build request");

            let client = self.client.clone();
            let sentry_url = upstream.sentry_url.clone();
            let http_timeout_secs = self.timeouts.http_timeout_secs as u64;

            // Clone data we need to track even if task times out
            let public_keys_for_task = public_keys.clone();
            let cell_name_for_task = cell_name.clone();

            // Spawn a task for each upstream request
            let task = tokio::spawn(async move {
                let sentry_url_str = sentry_url.to_string();
                let result: Result<Response<Bytes>, IngestRouterError> =
                    send_to_upstream(&client, &sentry_url, request, http_timeout_secs).await;

                UpstreamTaskResult {
                    cell_name: cell_name_for_task,
                    sentry_url: sentry_url_str,
                    public_keys: public_keys_for_task,
                    result,
                }
            });

            // Store task handle and keys (keys needed if task times out)
            tasks.push(TaskWithKeys {
                handle: task,
                public_keys,
            });
        }

        Ok(tasks)
    }

    /// Collects results from all spawned tasks and merges them.
    ///
    /// Uses adaptive timeouts with cutoff strategy:
    /// - Initial: Wait up to task_initial_timeout_secs for first upstream to respond
    /// - Subsequent: Once first succeeds, ALL remaining tasks have task_subsequent_timeout_secs
    ///   total (from first success) to complete. This prevents slow/down cells from blocking
    ///   progress when we already have good data.
    ///
    /// Failed keys are added to the pending array for retry.
    ///
    /// Global config is selected from the highest priority cell based on cells.cell_list order.
    async fn collect_and_merge(&self, tasks: Vec<TaskWithKeys>, cells: &Cells) -> MergedResults {
        let mut merged_configs = HashMap::new();
        let mut merged_pending = Vec::new();
        let mut extra_by_cell: HashMap<String, HashMap<String, JsonValue>> = HashMap::new();
        let mut headers_by_cell: HashMap<String, HeaderMap> = HashMap::new();
        let mut additional_deadline: Option<Instant> = None;

        for task_with_keys in tasks {
            let TaskWithKeys {
                mut handle,
                public_keys,
            } = task_with_keys;

            // Adaptive timeout
            // - Before first success: Use task_initial_timeout_secs
            // - After first success: Use remaining time until deadline
            let timeout_duration = if let Some(deadline) = additional_deadline {
                // Calculate remaining time to deadline
                let now = Instant::now();
                if now >= deadline {
                    // Deadline already passed, use minimal timeout
                    Duration::from_millis(1)
                } else {
                    deadline.duration_since(now)
                }
            } else {
                Duration::from_secs(self.timeouts.task_initial_timeout_secs as u64)
            };

            let task_result = timeout(timeout_duration, &mut handle).await;

            // Handle timeout
            let Ok(join_result) = task_result else {
                tracing::error!(
                    "Task timed out after {} seconds",
                    timeout_duration.as_secs()
                );
                // Abort the timed-out task to prevent it from continuing in background
                handle.abort();
                // Add all keys from this upstream to pending (v3 protocol)
                merged_pending.extend(public_keys);
                continue;
            };

            // Handle task panic
            let Ok(upstream_result) = join_result else {
                if let Err(e) = join_result {
                    tracing::error!("Task panicked: {e}");
                }
                // Add all keys from this upstream to pending (v3 protocol)
                merged_pending.extend(public_keys);
                continue;
            };

            // Process the upstream result
            let result_had_configs = upstream_result.result.is_ok();
            let cell_name_from_result = upstream_result.cell_name.clone();
            if let Some((extra, headers)) = self.process_upstream_result(
                upstream_result,
                &mut merged_configs,
                &mut merged_pending,
            ) {
                // Store extra fields and headers by cell name for later priority-based selection
                extra_by_cell.insert(cell_name_from_result.clone(), extra);
                headers_by_cell.insert(cell_name_from_result, headers);
            }

            // Set deadline on first success
            if result_had_configs && additional_deadline.is_none() {
                additional_deadline = Some(
                    Instant::now()
                        + Duration::from_secs(self.timeouts.task_subsequent_timeout_secs as u64),
                );
            }
        }

        // Select global config and headers from highest priority cell by iterating cells.cell_list
        // cell_list is already in priority order (first = highest priority)
        let (merged_extra, merged_headers) = cells
            .cell_list
            .iter()
            .find_map(|cell_name| {
                extra_by_cell.get(cell_name).cloned().map(|extra| {
                    let headers = headers_by_cell.get(cell_name).cloned().unwrap_or_default();
                    (extra, headers)
                })
            })
            .unwrap_or_default();

        MergedResults {
            configs: merged_configs,
            pending: merged_pending,
            extra: merged_extra,
            headers: merged_headers,
        }
    }

    /// Processes the result from a single upstream task.
    ///
    /// Handles both successful and failed upstream responses, merging successful
    /// configs and adding failed keys to the pending array.
    ///
    /// Returns the extra fields (global config, etc.) and headers if the request succeeded,
    /// which allows the caller to select based on cell priority.
    fn process_upstream_result(
        &self,
        upstream_result: UpstreamTaskResult,
        merged_configs: &mut HashMap<String, JsonValue>,
        merged_pending: &mut Vec<String>,
    ) -> Option<(HashMap<String, JsonValue>, HeaderMap)> {
        let UpstreamTaskResult {
            cell_name: _,
            sentry_url,
            public_keys,
            result,
        } = upstream_result;

        // Handle HTTP request failure
        let Ok(response) = result else {
            tracing::error!(
                sentry_url = %sentry_url,
                error = %result.unwrap_err(),
                "Request to upstream failed"
            );
            // Add all keys from this upstream to pending (v3 protocol)
            merged_pending.extend(public_keys);
            return None;
        };

        // Extract headers before consuming the response
        let (parts, body) = response.into_parts();
        let headers = parts.headers;

        // Parse response body
        match RelayProjectConfigsResponse::from_bytes(&body) {
            Ok(response_data) => {
                // Merge configs from this upstream
                merged_configs.extend(response_data.configs);

                // Merge pending arrays (v3 protocol)
                if let Some(pending) = response_data.pending {
                    merged_pending.extend(pending);
                }

                // Return extra fields and headers for priority-based selection
                Some((response_data.extra, headers))
            }
            Err(e) => {
                tracing::error!(
                    sentry_url = %sentry_url,
                    error = %e,
                    "Failed to parse response from upstream"
                );
                // Add all keys from this upstream to pending
                merged_pending.extend(public_keys);
                None
            }
        }
    }
}

/// Handler for Relay Project Configs endpoint.
///
/// See module-level docs for complete protocol details, implementation strategy,
/// and request/response flow diagrams.
pub struct RelayProjectConfigsHandler {
    /// Executor for spawning and collecting upstream tasks.
    executor: UpstreamTaskExecutor,

    /// Locales mapping for locale-based upstream lookups.
    ///
    /// Maps locale → cell name → upstream (relay URL + sentry URL).
    locales: Locales,
}

impl RelayProjectConfigsHandler {
    pub fn new(locales_config: HashMap<String, Vec<CellConfig>>, timeouts: RelayTimeouts) -> Self {
        let connector = HttpConnector::new();
        let client = Client::builder(TokioExecutor::new()).build(connector);

        // Build locales from config
        let locales = Locales::new(locales_config);

        // Create executor for task management
        let executor = UpstreamTaskExecutor::new(client, timeouts);

        Self { executor, locales }
    }

    pub async fn handle<B>(
        &self,
        locale: &str,
        request: Request<B>,
    ) -> Result<Response<BoxBody<Bytes, IngestRouterError>>, IngestRouterError>
    where
        B: Body + Send + 'static,
        B::Data: Send,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        // Get cells for this locale
        let cells = self.locales.get_cells(locale).ok_or_else(|| {
            IngestRouterError::InternalError(format!(
                "No targets configured for locale: {}",
                locale
            ))
        })?;

        // Buffer the request body. We need to full body in order to do request massaging.
        let (parts, body) = request.into_parts();
        let body_bytes = body
            .collect()
            .await
            .map(|collected| collected.to_bytes())
            .map_err(|e| IngestRouterError::RequestBodyError(e.to_string()))?;
        let base_request = Request::from_parts(parts, ());

        // Process the request
        self.handle_with_targets(cells, base_request, body_bytes)
            .await
    }

    /// Internal method that orchestrates the split-merge flow.
    ///
    /// High-level steps:
    /// 1. Parse and split the request across upstreams
    /// 2. Spawn parallel tasks to fan out requests
    /// 3. Collect and merge results from all upstreams
    /// 4. Build and return the merged response
    async fn handle_with_targets(
        &self,
        cells: &Cells,
        base_request: Request<()>,
        body_bytes: Bytes,
    ) -> Result<Response<BoxBody<Bytes, IngestRouterError>>, IngestRouterError> {
        // Parse the request
        let request_data = RelayProjectConfigsRequest::from_bytes(&body_bytes).map_err(|e| {
            IngestRouterError::RequestBodyError(format!("Failed to parse request: {e}"))
        })?;

        // Split publicKeys across upstreams
        let split_requests = self.split_keys_by_upstream(&request_data, cells);

        // Spawn tasks to fan out requests in parallel
        let tasks = self.executor.spawn_tasks(split_requests, &base_request)?;

        // Collect and merge results from all tasks (uses cells.cell_list priority order)
        let results = self.executor.collect_and_merge(tasks, cells).await;

        // Only return an error if we have no configs AND no pending AND we had keys to request
        // Having keys in pending is a valid v3 response (relay will retry later)
        // Return 503 Service Unavailable to indicate temporary unavailability
        if results.configs.is_empty()
            && results.pending.is_empty()
            && !request_data.public_keys.is_empty()
        {
            return Err(IngestRouterError::ServiceUnavailable(
                "All upstream cells are unavailable".to_string(),
            ));
        }

        // Build merged response in the relay format
        self.build_merged_response(
            results.configs,
            results.pending,
            results.extra,
            results.headers,
        )
    }

    /// Splits public keys across multiple upstream cells.
    ///
    /// Current: Round-robin stub. TODO: Replace with locator service lookup.
    /// See module-level docs for complete splitting strategy and global config handling.
    fn split_keys_by_upstream(
        &self,
        request: &RelayProjectConfigsRequest,
        cells: &Cells,
    ) -> Vec<SplitRequest> {
        let cell_list = &cells.cell_list;
        if cell_list.is_empty() {
            return Vec::new();
        }

        let mut split: HashMap<String, Vec<String>> = HashMap::new();

        // TODO: Replace with control plane lookup per key
        for (index, public_key) in request.public_keys.iter().enumerate() {
            let cell_name = &cell_list[index % cell_list.len()];

            split
                .entry(cell_name.clone())
                .or_default()
                .push(public_key.clone());
        }

        // Build a request for each cell with its assigned publicKeys
        split
            .into_iter()
            .map(|(cell_name, public_keys)| {
                let upstream = cells
                    .cell_to_upstreams
                    .get(&cell_name)
                    .expect("Cell name in list must exist in HashMap");

                // All fields in extra (including global, noCache, etc.) are passed through as-is
                SplitRequest {
                    cell_name,
                    upstream: upstream.clone(),
                    request: RelayProjectConfigsRequest {
                        public_keys,
                        extra: request.extra.clone(),
                    },
                }
            })
            .collect()
    }

    /// Build a merged response from collected configs in relay format.
    ///
    /// Uses headers from the highest priority cell (same cell used for global config).
    /// Filters out hop-by-hop headers and Content-Length (which is recalculated for the new body).
    fn build_merged_response(
        &self,
        merged_configs: HashMap<String, JsonValue>,
        merged_pending: Vec<String>,
        merged_extra: HashMap<String, JsonValue>,
        mut upstream_headers: HeaderMap,
    ) -> Result<Response<BoxBody<Bytes, IngestRouterError>>, IngestRouterError> {
        // Wrap in relay response format
        let response = RelayProjectConfigsResponse {
            configs: merged_configs,
            pending: if merged_pending.is_empty() {
                None
            } else {
                Some(merged_pending)
            },
            extra: merged_extra,
        };

        let merged_json = serde_json::to_vec(&response)
            .map_err(|e| IngestRouterError::ResponseSerializationError(e.to_string()))?;

        // Filter hop-by-hop headers from upstream (assumes HTTP/1.1)
        // These headers are connection-specific and shouldn't be forwarded
        shared::http::filter_hop_by_hop(&mut upstream_headers, hyper::Version::HTTP_11);

        // Remove Content-Length since we're creating a new body with different length
        upstream_headers.remove(CONTENT_LENGTH);

        // Build response with filtered headers from highest priority cell
        let mut builder = Response::builder().status(StatusCode::OK);

        // Copy filtered headers from upstream
        for (name, value) in upstream_headers.iter() {
            builder = builder.header(name, value);
        }

        // Always set/override Content-Type to ensure it's correct
        builder = builder.header(CONTENT_TYPE, "application/json");

        builder
            .body(
                Full::new(Bytes::from(merged_json))
                    .map_err(|e| match e {})
                    .boxed(),
            )
            .map_err(|e| IngestRouterError::ResponseBuildError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    #[tokio::test]
    async fn test_response_building_and_v3_protocol() {
        let handler = RelayProjectConfigsHandler::new(HashMap::new(), RelayTimeouts::default());

        // Test: Empty response
        let response = handler
            .build_merged_response(HashMap::new(), Vec::new(), HashMap::new(), HeaderMap::new())
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(parsed, serde_json::json!({"configs": {}}));

        // Test: Multiple configs with field preservation
        let mut configs = HashMap::new();
        configs.insert(
            "project1".to_string(),
            serde_json::json!({
                "disabled": false,
                "slug": "test-project",
                "organizationId": 42,
                "projectId": 100,
                "customField": "customValue"
            }),
        );
        configs.insert(
            "project2".to_string(),
            serde_json::json!({"config": "value2"}),
        );

        let response = handler
            .build_merged_response(configs, Vec::new(), HashMap::new(), HeaderMap::new())
            .unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert!(parsed["configs"].get("project1").is_some());
        assert!(parsed["configs"].get("project2").is_some());
        assert_eq!(parsed["configs"]["project1"]["disabled"], false);
        assert_eq!(parsed["configs"]["project1"]["slug"], "test-project");
        assert_eq!(parsed["configs"]["project1"]["customField"], "customValue");

        // Test: V3 protocol - Pending array with values
        let pending = vec!["key1".to_string(), "key2".to_string(), "key3".to_string()];
        let response = handler
            .build_merged_response(HashMap::new(), pending, HashMap::new(), HeaderMap::new())
            .unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(parsed["pending"].as_array().unwrap().len(), 3);
        assert_eq!(parsed["pending"][0], "key1");

        // Test: V3 protocol - Empty pending omitted
        let response = handler
            .build_merged_response(HashMap::new(), Vec::new(), HashMap::new(), HeaderMap::new())
            .unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert!(parsed.get("pending").is_none());

        // Test: V3 protocol - Global config and extra fields
        let mut extra = HashMap::new();
        extra.insert(
            "global".to_string(),
            serde_json::json!({"measurements": {"maxCustomMeasurements": 10}}),
        );
        extra.insert("global_status".to_string(), serde_json::json!("ready"));
        extra.insert(
            "futureFeature".to_string(),
            serde_json::json!({"enabled": true}),
        );

        let response = handler
            .build_merged_response(HashMap::new(), Vec::new(), extra, HeaderMap::new())
            .unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            parsed["global"]["measurements"]["maxCustomMeasurements"],
            10
        );
        assert_eq!(parsed["global_status"], "ready");
        assert_eq!(parsed["futureFeature"]["enabled"], true);
    }

    #[tokio::test]
    async fn test_all_upstreams_fail_returns_error() {
        // Set up handler with invalid upstreams
        let mut locales = HashMap::new();
        locales.insert(
            "us".to_string(),
            vec![CellConfig {
                name: "us-cell-1".to_string(),
                relay_url: Url::parse("http://invalid-relay.example.com:8080").unwrap(),
                sentry_url: Url::parse("http://invalid-sentry.example.com:8080").unwrap(),
            }],
        );

        let handler = RelayProjectConfigsHandler::new(locales, RelayTimeouts::default());

        // Create a request with public keys
        let request_body = serde_json::json!({
            "publicKeys": ["test-key-1", "test-key-2"]
        });
        let body_bytes = Bytes::from(serde_json::to_vec(&request_body).unwrap());

        // Create empty cells to simulate all upstreams failing
        let empty_cells = Cells {
            cell_list: Vec::new(),
            cell_to_upstreams: HashMap::new(),
        };

        // Create a base request
        let request = Request::builder().method("POST").uri("/").body(()).unwrap();

        // This should return an error since no upstreams will succeed (empty targets)
        let result = handler
            .handle_with_targets(&empty_cells, request, body_bytes)
            .await;

        assert!(result.is_err());
        match result {
            Err(IngestRouterError::ServiceUnavailable(msg)) => {
                assert_eq!(msg, "All upstream cells are unavailable");
                // Verify the error maps to 503 status code
                let error = IngestRouterError::ServiceUnavailable(msg);
                assert_eq!(error.status_code(), hyper::StatusCode::SERVICE_UNAVAILABLE);
            }
            _ => panic!("Expected ServiceUnavailable error"),
        }
    }

    #[tokio::test]
    async fn test_v3_upstream_failure_adds_keys_to_pending() {
        // Set up handler with invalid upstream that will fail to connect
        let mut locales = HashMap::new();
        locales.insert(
            "us".to_string(),
            vec![CellConfig {
                name: "us-cell-1".to_string(),
                relay_url: Url::parse("http://localhost:1").unwrap(), // Invalid port
                sentry_url: Url::parse("http://localhost:1").unwrap(), // Will fail to connect
            }],
        );

        let handler = RelayProjectConfigsHandler::new(locales.clone(), RelayTimeouts::default());

        // Create a request with public keys
        let request_body = serde_json::json!({
            "publicKeys": ["test-key-1", "test-key-2", "test-key-3"]
        });
        let body_bytes = Bytes::from(serde_json::to_vec(&request_body).unwrap());

        // Get cells
        let locales = Locales::new(locales);
        let cells = locales.get_cells("us").unwrap();

        // Create a base request
        let request = Request::builder().method("POST").uri("/").body(()).unwrap();

        // When upstream fails, all its keys should be added to pending
        let result = handler
            .handle_with_targets(cells, request, body_bytes)
            .await;

        // Should succeed (v3 protocol - returning pending is valid)
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Parse response
        let body = response.into_body();
        let body_bytes = body.collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

        // Verify configs is empty (upstream failed)
        assert!(parsed["configs"].as_object().unwrap().is_empty());

        // Verify all keys are in pending
        assert!(parsed.get("pending").is_some());
        let pending = parsed["pending"].as_array().unwrap();
        assert_eq!(pending.len(), 3);
        assert!(pending.contains(&serde_json::json!("test-key-1")));
        assert!(pending.contains(&serde_json::json!("test-key-2")));
        assert!(pending.contains(&serde_json::json!("test-key-3")));
    }

    #[test]
    fn test_extra_fields_passthrough() {
        // Test 1: Split behavior - all upstreams get the same extra fields (including global)
        let mut locales = HashMap::new();
        locales.insert(
            "us".to_string(),
            vec![
                CellConfig {
                    name: "us-cell-1".to_string(),
                    relay_url: Url::parse("http://us1-relay:8080").unwrap(),
                    sentry_url: Url::parse("http://us1-sentry:8080").unwrap(),
                },
                CellConfig {
                    name: "us-cell-2".to_string(),
                    relay_url: Url::parse("http://us2-relay:8080").unwrap(),
                    sentry_url: Url::parse("http://us2-sentry:8080").unwrap(),
                },
            ],
        );

        let handler = RelayProjectConfigsHandler::new(locales.clone(), RelayTimeouts::default());
        let mut extra = HashMap::new();
        extra.insert("global".to_string(), serde_json::json!(true));
        let request = RelayProjectConfigsRequest {
            public_keys: vec!["key1".to_string(), "key2".to_string()],
            extra,
        };

        let locales_obj = Locales::new(locales);
        let cells = locales_obj.get_cells("us").unwrap();
        let splits = handler.split_keys_by_upstream(&request, cells);

        assert_eq!(splits.len(), 2);
        // All upstreams get the same extra fields (including global: true)
        assert_eq!(
            splits[0].request.extra.get("global"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            splits[1].request.extra.get("global"),
            Some(&serde_json::json!(true))
        );

        // Test 2: Serialization - extra fields are included
        let request = RelayProjectConfigsRequest {
            public_keys: vec!["key1".to_string()],
            extra: HashMap::new(),
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: JsonValue = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("global").is_none());
        assert!(parsed.get("publicKeys").is_some());
    }

    #[tokio::test]
    async fn test_headers_from_highest_priority_cell() {
        use hyper::header::{CACHE_CONTROL, HeaderValue};

        let mut locales = HashMap::new();
        locales.insert(
            "test".to_string(),
            vec![CellConfig {
                name: "test-cell".to_string(),
                relay_url: Url::parse("http://localhost:8090").unwrap(),
                sentry_url: Url::parse("http://localhost:8080").unwrap(),
            }],
        );

        let handler = RelayProjectConfigsHandler::new(locales, RelayTimeouts::default());

        // Create headers from upstream (simulating what we'd get from highest priority cell)
        let mut upstream_headers = HeaderMap::new();
        upstream_headers.insert(CACHE_CONTROL, HeaderValue::from_static("max-age=300"));
        upstream_headers.insert(
            "X-Sentry-Rate-Limit-Remaining",
            HeaderValue::from_static("100"),
        );

        // Add a hop-by-hop header that should be filtered
        upstream_headers.insert(
            hyper::header::CONNECTION,
            HeaderValue::from_static("keep-alive"),
        );

        let response = handler
            .build_merged_response(HashMap::new(), Vec::new(), HashMap::new(), upstream_headers)
            .unwrap();

        // Verify headers are copied
        assert_eq!(
            response.headers().get(CACHE_CONTROL),
            Some(&HeaderValue::from_static("max-age=300"))
        );
        assert_eq!(
            response.headers().get("X-Sentry-Rate-Limit-Remaining"),
            Some(&HeaderValue::from_static("100"))
        );

        // Verify hop-by-hop headers are filtered out
        assert!(response.headers().get(hyper::header::CONNECTION).is_none());

        // Verify Content-Type is always set
        assert_eq!(
            response.headers().get(CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json"))
        );
    }
}
