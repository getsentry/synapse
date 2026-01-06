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
//! The endpoint implementation is at <https://github.com/getsentry/sentry/blob/master/src/sentry/api/endpoints/relay/project_configs.py>
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
//!    - Query locator service for each public key to get cell name
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

use crate::api::utils::{deserialize_body, normalize_headers, serialize_to_body};
use crate::errors::IngestRouterError;
use crate::handler::{CellId, ExecutionMode, Handler, SplitMetadata};
use crate::locale::Cells;
use async_trait::async_trait;
use http::StatusCode;
use http::response::Parts;
use hyper::body::Bytes;
use hyper::header::{CONTENT_TYPE, HeaderValue};
use hyper::{Request, Response};
use locator::client::Locator;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use shared::http::make_error_response;
use std::collections::HashMap;

/// Request format for the relay project configs endpoint.
///
/// # Example
/// ```json
/// {
///   "publicKeys": ["key1", "key2", "key3"],
///   "noCache": false,
///   "global": true
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfigsRequest {
    /// DSN public keys to fetch configs for.
    #[serde(rename = "publicKeys")]
    pub public_keys: Vec<String>,

    /// Other fields (`global`, `noCache`, future fields) for forward compatibility.
    /// All fields are passed through as-is to upstreams.
    #[serde(flatten)]
    pub extra_fields: HashMap<String, JsonValue>,
}

/// Response format for the relay project configs endpoint.
///
/// # Example
/// ```json
/// {
///   "configs": {
///     "key1": {
///       "disabled": false,
///       "slug": "project-slug",
///       "publicKeys": [...],
///       "config": {...},
///       "organizationId": 1,
///       "projectId": 42
///     }
///   },
///   "pending": ["key2", "key3"],
///   "global": {...},
///   "global_status": "ready"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfigsResponse {
    /// Project configs (HashMap merged from all upstreams).
    #[serde(rename = "configs")]
    pub project_configs: HashMap<String, JsonValue>,

    /// Keys being computed async or from failed upstreams (concatenated).
    #[serde(rename = "pending", skip_serializing_if = "Vec::is_empty", default)]
    pub pending_keys: Vec<String>,

    /// Other fields (`global`, `global_status`, future fields).
    #[serde(flatten)]
    pub extra_fields: HashMap<String, JsonValue>,
}

impl ProjectConfigsResponse {
    pub fn new() -> Self {
        Self {
            project_configs: HashMap::new(),
            pending_keys: Vec::new(),
            extra_fields: HashMap::new(),
        }
    }
}

impl Default for ProjectConfigsResponse {
    fn default() -> Self {
        Self::new()
    }
}

// Handler implementation for the Relay Project Configs endpoint

#[derive(Default, Debug)]
struct ProjectConfigsMetadata {
    // keys that are assigned to a cell
    cell_to_keys: HashMap<CellId, Vec<String>>,
    // keys that couldn't be assigned to any cell
    unassigned_keys: Vec<String>,
}

/// Handler for the Relay Project Configs endpoint
///
/// Routes public keys to cells using the locator service, splits requests
/// across cells, and merges responses with proper handling of failures and
/// pending keys.
pub struct ProjectConfigsHandler {
    locator: Locator,
}

impl ProjectConfigsHandler {
    pub fn new(locator: Locator) -> Self {
        Self { locator }
    }
}

#[async_trait]
impl Handler for ProjectConfigsHandler {
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }
    async fn split_request(
        &self,
        request: Request<Bytes>,
        cells: &Cells,
    ) -> Result<(Vec<(CellId, Request<Bytes>)>, SplitMetadata), IngestRouterError> {
        let (mut parts, body) = request.into_parts();
        let parsed: ProjectConfigsRequest = deserialize_body(body)?;
        normalize_headers(&mut parts.headers, parts.version);

        let public_keys = parsed.public_keys;
        let extra_fields = parsed.extra_fields;

        // Route each public key to its owning cell using the locator service
        let mut cell_to_keys: HashMap<CellId, Vec<String>> = HashMap::new();
        let mut pending: Vec<String> = Vec::new();

        for public_key in public_keys {
            match self
                .locator
                .lookup(&public_key, Some(cells.locality()))
                .await
            {
                Ok(cell_id) => {
                    cell_to_keys.entry(cell_id).or_default().push(public_key);
                }
                Err(e) => {
                    // Locator errors, add to pending
                    tracing::error!(
                        public_key = %public_key,
                        error = ?e,
                        "Failed to route public key"
                    );
                    pending.push(public_key);
                }
            }
        }

        let cell_requests = cell_to_keys
            .iter()
            .map(|(cell_id, keys)| {
                let project_configs_request = ProjectConfigsRequest {
                    public_keys: keys.clone(),
                    extra_fields: extra_fields.clone(),
                };

                let body = serialize_to_body(&project_configs_request)?;
                let req = Request::from_parts(parts.clone(), body);
                Ok((cell_id.into(), req))
            })
            .collect::<Result<_, IngestRouterError>>()?;

        let metadata = Box::new(ProjectConfigsMetadata {
            cell_to_keys,
            unassigned_keys: pending,
        });
        Ok((cell_requests, metadata))
    }

    async fn merge_responses(
        &self,
        responses: Vec<(CellId, Result<Response<Bytes>, IngestRouterError>)>,
        metadata: SplitMetadata,
    ) -> Response<Bytes> {
        // TODO: Consider refactoring to avoid runtime downcast
        let meta = metadata
            .downcast::<ProjectConfigsMetadata>()
            .unwrap_or(Box::new(ProjectConfigsMetadata::default()));

        let mut merged = ProjectConfigsResponse::new();
        merged.pending_keys.extend(meta.unassigned_keys);

        // Order the responses so successful ones come first
        let sorted_responses = {
            let mut sorted = responses;
            sorted.sort_by_key(|(_, result)| match result {
                Ok(r) if r.status().is_success() => 0,
                Ok(_) => 1,
                Err(_) => 2,
            });
            sorted
        };

        // True if at least one response is ok
        let has_successful_response = sorted_responses
            .first()
            .is_some_and(|(_, r)| r.as_ref().ok().is_some_and(|r| r.status().is_success()));

        // Parts is populated from the first response.
        let mut parts: Option<Parts> = None;

        for (cell_id, result) in sorted_responses {
            let successful_response = result.ok().filter(|r| r.status().is_success());

            let Some(response) = successful_response else {
                // Any failure adds the cell's keys to pending
                if let Some(keys) = meta.cell_to_keys.get(&cell_id) {
                    merged.pending_keys.extend(keys.clone());
                }
                continue;
            };

            let (p, body) = response.into_parts();
            if parts.is_none() {
                parts = Some(p);
            }

            if let Ok(parsed) = deserialize_body::<ProjectConfigsResponse>(body) {
                merged.project_configs.extend(parsed.project_configs);
                merged.extra_fields.extend(parsed.extra_fields);
                merged.pending_keys.extend(parsed.pending_keys);
            } else {
                tracing::error!(
                    cell_id = %cell_id,
                    "Failed to deserialize project configs response from cell"
                );
            }
        }

        let serialized_body = serialize_to_body(&merged);

        match (has_successful_response, parts, serialized_body) {
            (true, Some(mut p), Ok(body)) => {
                normalize_headers(&mut p.headers, p.version);
                p.headers
                    .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                return Response::from_parts(p, body);
            }
            (_, Some(p), _) => make_error_response(p.status),
            (_, _, _) => make_error_response(StatusCode::BAD_GATEWAY),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;
    use crate::locale::Locales;
    use crate::testutils::create_test_locator;
    use std::collections::HashMap;
    use url::Url;

    fn build_request(project_configs_request: ProjectConfigsRequest) -> Request<Bytes> {
        let body = serialize_to_body(&project_configs_request).unwrap();
        Request::builder()
            .method("POST")
            .uri("/api/0/relays/projectconfigs/")
            .body(body)
            .unwrap()
    }

    fn build_response(project_configs_response: serde_json::Value) -> Response<Bytes> {
        let body = serialize_to_body(&project_configs_response).unwrap();
        Response::builder().status(200).body(body).unwrap()
    }

    #[tokio::test]
    async fn test_split_request_multiple_cells() {
        let key_to_cell = HashMap::from([
            ("key1".to_string(), "us1".to_string()),
            ("key2".to_string(), "us2".to_string()),
            ("key3".to_string(), "us1".to_string()),
        ]);
        let locator = create_test_locator(key_to_cell).await;

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

        let locales_obj = Locales::new(locales);
        let cells = locales_obj.get_cells("us").unwrap();

        let handler = ProjectConfigsHandler::new(locator);

        let mut extra = HashMap::new();
        extra.insert("global".to_string(), serde_json::json!(true));

        let request = build_request(ProjectConfigsRequest {
            public_keys: vec!["key1".to_string(), "key2".to_string(), "key3".to_string()],
            extra_fields: extra.clone(),
        });

        let (cell_requests, _metadata) = handler.split_request(request, &cells).await.unwrap();

        // Should have 2 cell requests (us1 and us2)
        assert_eq!(cell_requests.len(), 2);

        // Find us1 and us2 requests and parse their bodies
        let (us1_id, us1_req) = cell_requests
            .into_iter()
            .find(|(id, _)| id == "us1")
            .unwrap();

        let us1_body: ProjectConfigsRequest = deserialize_body(us1_req.into_body()).unwrap();

        let key_to_cell = HashMap::from([
            ("key1".to_string(), "us1".to_string()),
            ("key2".to_string(), "us2".to_string()),
            ("key3".to_string(), "us1".to_string()),
        ]);
        let locator = create_test_locator(key_to_cell).await;
        let handler = ProjectConfigsHandler::new(locator);
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
        let locales_obj = Locales::new(locales);
        let cells = locales_obj.get_cells("us").unwrap();
        let request2 = build_request(ProjectConfigsRequest {
            public_keys: vec!["key1".to_string(), "key2".to_string(), "key3".to_string()],
            extra_fields: extra.clone(),
        });
        let (cell_requests2, metadata) = handler.split_request(request2, &cells).await.unwrap();
        let (_, us2_req) = cell_requests2
            .into_iter()
            .find(|(id, _)| id == "us2")
            .unwrap();

        let us2_body: ProjectConfigsRequest = deserialize_body(us2_req.into_body()).unwrap();

        // Verify us1 has key1 and key3
        assert_eq!(us1_id, "us1");
        assert_eq!(us1_body.public_keys.len(), 2);
        assert!(us1_body.public_keys.contains(&"key1".to_string()));
        assert!(us1_body.public_keys.contains(&"key3".to_string()));
        assert_eq!(us1_body.extra_fields, extra);

        // Verify us2 has key2
        assert_eq!(us2_body.public_keys.len(), 1);
        assert!(us2_body.public_keys.contains(&"key2".to_string()));
        assert_eq!(us2_body.extra_fields, extra);

        let meta = metadata
            .downcast::<ProjectConfigsMetadata>()
            .unwrap_or(Box::new(ProjectConfigsMetadata::default()));
        assert!(meta.unassigned_keys.is_empty());
    }

    #[tokio::test]
    async fn test_split_request_unknown_key_goes_to_pending() {
        let key_to_cell = HashMap::from([("key1".to_string(), "us1".to_string())]);
        let locator = create_test_locator(key_to_cell).await;
        let locales = HashMap::from([(
            "us".to_string(),
            vec![CellConfig {
                id: "us1".to_string(),
                sentry_url: Url::parse("http://us1:8080").unwrap(),
                relay_url: Url::parse("http://us1:8090").unwrap(),
            }],
        )]);

        let locales_obj = Locales::new(locales);
        let cells = locales_obj.get_cells("us").unwrap();

        let handler = ProjectConfigsHandler::new(locator);

        let request = build_request(ProjectConfigsRequest {
            public_keys: vec!["key1".to_string(), "unknown_key".to_string()],
            extra_fields: HashMap::new(),
        });

        let (cell_requests, metadata) = handler.split_request(request, &cells).await.unwrap();

        // Should have 1 cell request (us1 with key1)
        assert_eq!(cell_requests.len(), 1);
        assert_eq!(cell_requests[0].0, "us1");

        // unknown key in metadata
        let meta = metadata
            .downcast::<ProjectConfigsMetadata>()
            .unwrap_or(Box::new(ProjectConfigsMetadata::default()));
        assert_eq!(meta.unassigned_keys, Vec::from(["unknown_key".to_string()]));
    }

    #[tokio::test]
    async fn test_merge_results_successful_cells() {
        let locator = create_test_locator(HashMap::new()).await;
        let handler = ProjectConfigsHandler::new(locator);

        // Create response from us1 with key1 and global config
        let response1_json = serde_json::json!({
            "configs": {
                "key1": {"slug": "project1"}
            },
            "global": {"version": 1}
        });
        let response1 = build_response(response1_json);

        // Create response from us2 with key2 and different global config
        let response2_json = serde_json::json!({
            "configs": {
                "key2": {"slug": "project2"}
            },
            "global": {"version": 2}
        });
        let response2 = build_response(response2_json);

        let results = vec![
            ("us1".to_string(), Ok(response1)),
            ("us2".to_string(), Ok(response2)),
        ];

        let metadata: SplitMetadata = Box::new(Vec::<String>::new());
        let merged = handler.merge_responses(results, metadata).await;

        let parsed: ProjectConfigsResponse = deserialize_body(merged.into_body()).unwrap();

        assert!(parsed.project_configs.contains_key("key1"));
        assert!(parsed.project_configs.contains_key("key2"));
    }

    #[tokio::test]
    async fn test_merge_responses_with_pending() {
        let locator = create_test_locator(HashMap::new()).await;
        let handler = ProjectConfigsHandler::new(locator);

        // Test pending keys from split phase (routing failures, unknown keys)

        // Create response from us1 with successful config
        let response1_json = serde_json::json!({
            "configs": {
                "key1": {"slug": "project1"}
            }
        });
        let response1 = build_response(response1_json);

        // Create response from us2 with successful config
        let response2_json = serde_json::json!({
            "configs": {
                "key2": {"slug": "project2"}
            }
        });
        let response2 = build_response(response2_json);

        let results: Vec<(CellId, Result<Response<Bytes>, IngestRouterError>)> = vec![
            ("us1".to_string(), Ok(response1)),
            ("us2".to_string(), Ok(response2)),
        ];

        // Pending from split phase (routing failures)
        let pending_from_split: ProjectConfigsMetadata = ProjectConfigsMetadata {
            cell_to_keys: HashMap::new(),
            unassigned_keys: vec![
                "key_routing_failed".to_string(),
                "key_from_failed_cell1".to_string(),
                "key_from_failed_cell2".to_string(),
            ],
        };

        let metadata: SplitMetadata = Box::new(pending_from_split);
        let merged = handler.merge_responses(results, metadata).await;

        // Parse merged response body so we can assert on pending keys
        let parsed: ProjectConfigsResponse = deserialize_body(merged.into_body()).unwrap();

        // Should have pending keys from split phase
        assert_eq!(parsed.pending_keys.len(), 3);
        assert!(
            parsed
                .pending_keys
                .contains(&"key_routing_failed".to_string())
        );
        assert!(
            parsed
                .pending_keys
                .contains(&"key_from_failed_cell1".to_string())
        );
        assert!(
            parsed
                .pending_keys
                .contains(&"key_from_failed_cell2".to_string())
        );
    }
}
