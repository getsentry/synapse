//! Relay Project Configuration Handler
//!
//! This module implements the split-merge strategy for handling Sentry's
//! `/api/0/relays/projectconfigs/` endpoint across multiple upstream Sentry instances
//! in a multi-cell architecture.
//!
//! # Protocol Overview
//!
//! The Relay Project Configs endpoint (version 3) is used by Relay instances to fetch
//! project configurations needed to process events. This implementation acts as a proxy
//! that:
//! 1. Splits requests across multiple upstream Sentry cells based on project ownership
//! 2. Fans out requests in parallel to multiple upstreams
//! 3. Merges responses back into a single v3-compliant response
//! 4. Passes through all config data unchanged
//!
//! ## Endpoint Details
//!
//! - **Path**: `/api/0/relays/projectconfigs/`
//! - **Method**: `POST`
//! - **Protocol Version**: 3 (current)
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
//!    - First upstream gets `global: true` (or original value)
//!    - All other upstreams get `global: false`
//!    - Prevents complex global config merging (only one upstream returns it)
//!    - TODO: Add capability to send to both but return only from first. This would
//!      enable a failover mechanism.
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
//! ### Extra fields (HashMap merge)
//! - Merge `extra` fields (includes `global`, `global_status`, future fields)
//! - No conflicts expected (only first upstream has global config)
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
//! - If pending is empty: Return 500 error (no recoverable state)
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
//!     │  global: true}           │  global: false}
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
//! │     • Merge extra fields (global)    │
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
//! - Request to US2: `{"publicKeys": ["key2"], "global": false}`
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

use crate::config::CellConfig;
use crate::errors::IngestRouterError;
use crate::http::send_to_upstream;
use crate::locale::{Locales, Upstream};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::{Body, Bytes};
use hyper::header::CONTENT_TYPE;
use hyper::{Request, Response, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::timeout;

/// Request format for the relay project configs endpoint.
///
/// See module-level docs for full protocol details.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RelayProjectConfigsRequest {
    /// DSN public keys to fetch configs for.
    #[serde(rename = "publicKeys")]
    pub public_keys: Vec<String>,

    /// Whether to include global config (optional).
    ///
    /// first upstream gets original value, others get `Some(false)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global: Option<bool>,

    /// Other fields (`noCache`, future fields) for forward compatibility.
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

/// Handler for Relay Project Configs endpoint.
///
/// See module-level docs for complete protocol details, implementation strategy,
/// and request/response flow diagrams.
pub struct RelayProjectConfigsHandler {
    /// HTTP client for sending requests to upstream Sentry instances.
    client: Client<HttpConnector, Full<Bytes>>,

    /// Locales mapping for locale-based upstream lookups.
    ///
    /// Maps locale → cell name → upstream (relay URL + sentry URL).
    locales: Locales,
}

impl RelayProjectConfigsHandler {
    pub fn new(locales_config: HashMap<String, HashMap<String, CellConfig>>) -> Self {
        let connector = HttpConnector::new();
        let client = Client::builder(TokioExecutor::new()).build(connector);

        // Build locales from config
        let locales = Locales::new(locales_config);

        Self { client, locales }
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
        self.handle_with_targets(&cells.cell_to_upstreams, base_request, body_bytes)
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
        cell_upstreams: &HashMap<String, Upstream>,
        base_request: Request<()>,
        body_bytes: Bytes,
    ) -> Result<Response<BoxBody<Bytes, IngestRouterError>>, IngestRouterError> {
        // Parse the request
        let request_data = RelayProjectConfigsRequest::from_bytes(&body_bytes).map_err(|e| {
            IngestRouterError::RequestBodyError(format!("Failed to parse request: {e}"))
        })?;

        // Split publicKeys across upstreams
        let split_requests = self.split_keys_by_upstream(&request_data, cell_upstreams);

        // Spawn tasks to fan out requests in parallel
        let tasks = self.spawn_upstream_tasks(split_requests, &base_request)?;

        // Collect and merge results from all tasks
        let results = self.collect_task_results(tasks).await;

        // Only return an error if we have no configs AND no pending AND we had keys to request
        // Having keys in pending is a valid v3 response (relay will retry later)
        if results.configs.is_empty()
            && results.pending.is_empty()
            && !request_data.public_keys.is_empty()
        {
            return Err(IngestRouterError::InternalError(
                "All upstream requests failed with no recoverable state".to_string(),
            ));
        }

        // Build merged response in the relay format
        self.build_merged_response(results.configs, results.pending, results.extra)
    }

    /// Splits public keys across multiple upstream cells.
    ///
    /// Current: Round-robin stub. TODO: Replace with locator service lookup.
    /// See module-level docs for complete splitting strategy and global config handling.
    fn split_keys_by_upstream(
        &self,
        request: &RelayProjectConfigsRequest,
        cell_upstreams: &HashMap<String, Upstream>,
    ) -> Vec<(Upstream, RelayProjectConfigsRequest)> {
        if cell_upstreams.is_empty() {
            return Vec::new();
        }

        // For now, convert HashMap to Vec for round-robin stub
        // In the future, we'll use cell_upstreams.get(cell_name) directly
        let upstreams: Vec<&Upstream> = cell_upstreams.values().collect();

        let mut split: HashMap<usize, Vec<String>> = HashMap::new();

        // Round-robin stub: distribute publicKeys evenly across upstreams
        // TODO: Replace with control plane lookup per key
        for (index, public_key) in request.public_keys.iter().enumerate() {
            let upstream_index = index % upstreams.len();

            split
                .entry(upstream_index)
                .or_default()
                .push(public_key.clone());
        }

        // Build a request for each upstream with its assigned publicKeys
        // Sort by upstream_index to ensure deterministic ordering
        let mut sorted_split: Vec<_> = split.into_iter().collect();
        sorted_split.sort_by_key(|(upstream_index, _)| *upstream_index);

        sorted_split
            .into_iter()
            .map(|(upstream_index, public_keys)| {
                let upstream = upstreams[upstream_index].clone();

                // For v3 protocol: set global=true for only the first upstream (index 0)
                // This avoids needing to merge global configs from multiple responses
                let global = if upstream_index == 0 {
                    // First upstream (index 0): keep global flag as-is from original request
                    request.global
                } else {
                    // Other upstreams: explicitly set global=false
                    Some(false)
                };

                let split_request = RelayProjectConfigsRequest {
                    public_keys,
                    global,
                    extra: request.extra.clone(),
                };
                (upstream, split_request)
            })
            .collect()
    }

    /// Spawn async tasks to send requests to all upstreams in parallel.
    ///
    /// Each task sends the split request to its designated upstream and returns
    /// an `UpstreamTaskResult` containing the response or error.
    ///
    /// Returns tuples of (JoinHandle, public_keys) so keys can be added to pending
    /// if the task times out or panics.
    fn spawn_upstream_tasks(
        &self,
        split_requests: Vec<(Upstream, RelayProjectConfigsRequest)>,
        base_request: &Request<()>,
    ) -> Result<Vec<TaskWithKeys>, IngestRouterError> {
        let mut tasks = Vec::new();

        for (upstream, split_request) in split_requests {
            // Track public keys for this upstream (needed if request fails)
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

            // Clone keys so we can track them even if task times out
            let public_keys_for_task = public_keys.clone();

            // Spawn a task for each upstream request
            let task = tokio::spawn(async move {
                let sentry_url_str = sentry_url.to_string();
                let result: Result<Response<Bytes>, IngestRouterError> =
                    send_to_upstream(&client, &sentry_url, request, 30).await;

                UpstreamTaskResult {
                    sentry_url: sentry_url_str,
                    public_keys: public_keys_for_task,
                    result,
                }
            });

            // Store both task handle and keys (keys needed if task times out)
            tasks.push(TaskWithKeys {
                handle: task,
                public_keys,
            });
        }

        Ok(tasks)
    }

    /// Collect results from all spawned tasks and merge them.
    ///
    /// Handles timeouts, task panics, and HTTP failures gracefully.
    /// Failed keys are added to the pending array for retry.
    async fn collect_task_results(&self, tasks: Vec<TaskWithKeys>) -> MergedResults {
        let mut merged_configs = HashMap::new();
        let mut merged_pending = Vec::new();
        let mut merged_extra = HashMap::new();

        for task_with_keys in tasks {
            let TaskWithKeys {
                mut handle,
                public_keys,
            } = task_with_keys;
            let task_result = timeout(Duration::from_secs(30), &mut handle).await;

            // Handle timeout
            let Ok(join_result) = task_result else {
                tracing::error!("Task timed out after 30 seconds");
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
            self.process_upstream_result(
                upstream_result,
                &mut merged_configs,
                &mut merged_pending,
                &mut merged_extra,
            );
        }

        MergedResults {
            configs: merged_configs,
            pending: merged_pending,
            extra: merged_extra,
        }
    }

    /// Process the result from a single upstream task.
    ///
    /// Handles both successful and failed upstream responses, merging successful
    /// configs and adding failed keys to the pending array.
    fn process_upstream_result(
        &self,
        upstream_result: UpstreamTaskResult,
        merged_configs: &mut HashMap<String, JsonValue>,
        merged_pending: &mut Vec<String>,
        merged_extra: &mut HashMap<String, JsonValue>,
    ) {
        let UpstreamTaskResult {
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
            return;
        };

        // Parse response body
        let body = response.into_body();
        match RelayProjectConfigsResponse::from_bytes(&body) {
            Ok(response_data) => {
                // Merge configs from this upstream
                merged_configs.extend(response_data.configs);

                // Merge pending arrays (v3 protocol)
                if let Some(pending) = response_data.pending {
                    merged_pending.extend(pending);
                }

                // Merge extra fields (global, global_status, future fields)
                merged_extra.extend(response_data.extra);
            }
            Err(e) => {
                tracing::error!(
                    sentry_url = %sentry_url,
                    error = %e,
                    "Failed to parse response from upstream"
                );
                // Add all keys from this upstream to pending
                merged_pending.extend(public_keys);
            }
        }
    }

    /// Build a merged response from collected configs in relay format
    fn build_merged_response(
        &self,
        merged_configs: HashMap<String, JsonValue>,
        merged_pending: Vec<String>,
        merged_extra: HashMap<String, JsonValue>,
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

        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/json")
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
    async fn test_build_merged_response() {
        let handler = RelayProjectConfigsHandler::new(HashMap::new());

        // Test 1: Empty response
        let response = handler
            .build_merged_response(HashMap::new(), Vec::new(), HashMap::new())
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(parsed, serde_json::json!({"configs": {}}));

        // Test 2: Multiple configs with all fields preserved (pass-through verification)
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
            .build_merged_response(configs, Vec::new(), HashMap::new())
            .unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

        // Verify multiple configs present
        assert!(parsed["configs"].get("project1").is_some());
        assert!(parsed["configs"].get("project2").is_some());

        // Verify all fields preserved exactly as-is (pass-through)
        assert_eq!(parsed["configs"]["project1"]["disabled"], false);
        assert_eq!(parsed["configs"]["project1"]["slug"], "test-project");
        assert_eq!(parsed["configs"]["project1"]["organizationId"], 42);
        assert_eq!(parsed["configs"]["project1"]["projectId"], 100);
        assert_eq!(parsed["configs"]["project1"]["customField"], "customValue");
    }

    #[tokio::test]
    async fn test_all_upstreams_fail_returns_error() {
        // Set up handler with invalid upstreams
        let mut locales = HashMap::new();
        locales.insert(
            "us".to_string(),
            HashMap::from([(
                "us-cell-1".to_string(),
                CellConfig {
                    relay_url: Url::parse("http://invalid-relay.example.com:8080").unwrap(),
                    sentry_url: Url::parse("http://invalid-sentry.example.com:8080").unwrap(),
                },
            )]),
        );

        let handler = RelayProjectConfigsHandler::new(locales);

        // Create a request with public keys
        let request_body = serde_json::json!({
            "publicKeys": ["test-key-1", "test-key-2"]
        });
        let body_bytes = Bytes::from(serde_json::to_vec(&request_body).unwrap());

        // Create empty cell_targets to simulate all upstreams failing
        let empty_targets = HashMap::new();

        // Create a base request
        let request = Request::builder().method("POST").uri("/").body(()).unwrap();

        // This should return an error since no upstreams will succeed (empty targets)
        let result = handler
            .handle_with_targets(&empty_targets, request, body_bytes)
            .await;

        assert!(result.is_err());
        match result {
            Err(IngestRouterError::InternalError(msg)) => {
                assert_eq!(
                    msg,
                    "All upstream requests failed with no recoverable state"
                );
            }
            _ => panic!("Expected InternalError"),
        }
    }

    #[tokio::test]
    async fn test_v3_protocol_fields() {
        let handler = RelayProjectConfigsHandler::new(HashMap::new());

        // Test 1: Pending array with values
        let pending = vec!["key1".to_string(), "key2".to_string(), "key3".to_string()];
        let response = handler
            .build_merged_response(HashMap::new(), pending, HashMap::new())
            .unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(parsed["pending"].as_array().unwrap().len(), 3);
        assert_eq!(parsed["pending"][0], "key1");

        // Test 2: Empty pending omitted (skip_serializing_if)
        let response = handler
            .build_merged_response(HashMap::new(), Vec::new(), HashMap::new())
            .unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert!(parsed.get("pending").is_none());

        // Test 3: Global config and extra fields (forward compatibility)
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
            .build_merged_response(HashMap::new(), Vec::new(), extra)
            .unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

        // Verify global config
        assert_eq!(
            parsed["global"]["measurements"]["maxCustomMeasurements"],
            10
        );
        assert_eq!(parsed["global_status"], "ready");
        // Verify future/extra fields preserved
        assert_eq!(parsed["futureFeature"]["enabled"], true);
    }

    #[tokio::test]
    async fn test_v3_upstream_failure_adds_keys_to_pending() {
        // Set up handler with invalid upstream that will fail to connect
        let mut locales = HashMap::new();
        locales.insert(
            "us".to_string(),
            HashMap::from([(
                "us-cell-1".to_string(),
                CellConfig {
                    relay_url: Url::parse("http://localhost:1").unwrap(), // Invalid port
                    sentry_url: Url::parse("http://localhost:1").unwrap(), // Will fail to connect
                },
            )]),
        );

        let handler = RelayProjectConfigsHandler::new(locales.clone());

        // Create a request with public keys
        let request_body = serde_json::json!({
            "publicKeys": ["test-key-1", "test-key-2", "test-key-3"]
        });
        let body_bytes = Bytes::from(serde_json::to_vec(&request_body).unwrap());

        // Get upstreams
        let locales = Locales::new(locales);
        let cell_upstreams = &locales.get_cells("us").unwrap().cell_to_upstreams;

        // Create a base request
        let request = Request::builder().method("POST").uri("/").body(()).unwrap();

        // When upstream fails, all its keys should be added to pending
        let result = handler
            .handle_with_targets(cell_upstreams, request, body_bytes)
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
    fn test_global_field_handling() {
        // Test 1: Split behavior - first upstream gets true, others get false
        let mut locales = HashMap::new();
        locales.insert(
            "us".to_string(),
            HashMap::from([
                (
                    "us-cell-1".to_string(),
                    CellConfig {
                        relay_url: Url::parse("http://us1-relay:8080").unwrap(),
                        sentry_url: Url::parse("http://us1-sentry:8080").unwrap(),
                    },
                ),
                (
                    "us-cell-2".to_string(),
                    CellConfig {
                        relay_url: Url::parse("http://us2-relay:8080").unwrap(),
                        sentry_url: Url::parse("http://us2-sentry:8080").unwrap(),
                    },
                ),
            ]),
        );

        let handler = RelayProjectConfigsHandler::new(locales.clone());
        let request = RelayProjectConfigsRequest {
            public_keys: vec!["key1".to_string(), "key2".to_string()],
            global: Some(true),
            extra: HashMap::new(),
        };

        let locales_obj = Locales::new(locales);
        let cell_upstreams = &locales_obj.get_cells("us").unwrap().cell_to_upstreams;
        let splits = handler.split_keys_by_upstream(&request, cell_upstreams);

        assert_eq!(splits.len(), 2);
        assert_eq!(splits[0].1.global, Some(true)); // First gets true
        assert_eq!(splits[1].1.global, Some(false)); // Others get false

        // Test 2: Serialization - global omitted when None
        let request = RelayProjectConfigsRequest {
            public_keys: vec!["key1".to_string()],
            global: None,
            extra: HashMap::new(),
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: JsonValue = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("global").is_none());
        assert!(parsed.get("publicKeys").is_some());
    }
}
