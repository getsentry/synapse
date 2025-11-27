//! Response merging logic for combining upstream responses.
//!
//! This module handles merging responses from multiple upstream Sentry instances
//! following the v3 protocol merge strategy:
//! - Configs: HashMap merge (all configs from all upstreams)
//! - Pending: Array concatenation (includes failed keys)
//! - Extra fields: Priority-based selection (highest priority cell wins)

use crate::errors::IngestRouterError;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Bytes;
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderMap};
use hyper::{Response, StatusCode};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

use super::protocol::ProjectConfigsResponse;

/// Merged results from all upstream tasks.
pub struct MergedResults {
    /// All configs merged from successful upstreams.
    pub project_configs: HashMap<String, JsonValue>,
    /// All pending keys (from failed upstreams or upstream pending arrays).
    pub pending_keys: Vec<String>,
    /// Extra fields (global config, status, etc.).
    pub extra_fields: HashMap<String, JsonValue>,
    /// Headers from the highest priority upstream
    pub http_headers: HeaderMap,
}

impl MergedResults {
    /// Creates a new empty MergedResults.
    pub fn new() -> Self {
        Self {
            project_configs: HashMap::new(),
            pending_keys: Vec::new(),
            extra_fields: HashMap::new(),
            http_headers: HeaderMap::new(),
        }
    }

    /// Merges configs from a successful upstream response.
    pub fn merge_project_configs(&mut self, configs: HashMap<String, JsonValue>) {
        self.project_configs.extend(configs);
    }

    /// Adds keys to the pending array (for failed upstreams or retry).
    pub fn add_pending_keys(&mut self, keys: Vec<String>) {
        self.pending_keys.extend(keys);
    }

    /// Checks if results are empty (no configs, no pending, had keys to request).
    pub fn is_empty(&self, had_keys_to_request: bool) -> bool {
        self.project_configs.is_empty() && self.pending_keys.is_empty() && had_keys_to_request
    }

    /// Builds an HTTP response from the merged results.
    ///
    /// Uses headers from the highest priority cell (same cell used for global config).
    /// Filters out hop-by-hop headers and Content-Length (which is recalculated for the new body).
    pub fn into_response(
        mut self,
    ) -> Result<Response<BoxBody<Bytes, IngestRouterError>>, IngestRouterError> {
        let response = ProjectConfigsResponse {
            project_configs: self.project_configs,
            pending_keys: if self.pending_keys.is_empty() {
                None
            } else {
                Some(self.pending_keys)
            },
            extra_fields: self.extra_fields,
        };

        let merged_json = serde_json::to_vec(&response)
            .map_err(|e| IngestRouterError::ResponseSerializationError(e.to_string()))?;

        // Filter hop-by-hop headers (connection-specific, not forwarded)
        shared::http::filter_hop_by_hop(&mut self.http_headers, hyper::Version::HTTP_11);

        // Remove Content-Length since body size changed after merging
        self.http_headers.remove(CONTENT_LENGTH);

        let mut builder = Response::builder().status(StatusCode::OK);

        for (name, value) in self.http_headers.iter() {
            builder = builder.header(name, value);
        }

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

impl Default for MergedResults {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use hyper::header::{CACHE_CONTROL, HeaderValue};

    #[tokio::test]
    async fn test_empty_response() {
        let results = MergedResults::new();
        let response = results.into_response().unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(parsed, serde_json::json!({"configs": {}}));
    }

    #[tokio::test]
    async fn test_configs_merge() {
        let mut results = MergedResults::new();

        let mut configs1 = HashMap::new();
        configs1.insert(
            "key1".to_string(),
            serde_json::json!({"disabled": false, "slug": "project1"}),
        );

        let mut configs2 = HashMap::new();
        configs2.insert(
            "key2".to_string(),
            serde_json::json!({"disabled": false, "slug": "project2"}),
        );

        results.merge_project_configs(configs1);
        results.merge_project_configs(configs2);

        let response = results.into_response().unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(parsed["configs"].as_object().unwrap().len(), 2);
        assert!(parsed["configs"].get("key1").is_some());
        assert!(parsed["configs"].get("key2").is_some());
    }

    #[tokio::test]
    async fn test_pending_handling() {
        let mut results = MergedResults::new();

        results.add_pending_keys(vec!["key1".to_string(), "key2".to_string()]);
        results.add_pending_keys(vec!["key3".to_string()]);

        let response = results.into_response().unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(parsed["pending"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_empty_pending_omitted() {
        let results = MergedResults::new();
        let response = results.into_response().unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

        assert!(parsed.get("pending").is_none());
    }

    #[tokio::test]
    async fn test_headers_forwarding() {
        let mut results = MergedResults::new();

        // Add upstream headers
        results
            .http_headers
            .insert(CACHE_CONTROL, HeaderValue::from_static("max-age=300"));
        results.http_headers.insert(
            "X-Sentry-Rate-Limit-Remaining",
            HeaderValue::from_static("100"),
        );
        // Add hop-by-hop header that should be filtered
        results.http_headers.insert(
            hyper::header::CONNECTION,
            HeaderValue::from_static("keep-alive"),
        );

        let response = results.into_response().unwrap();

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

    #[tokio::test]
    async fn test_extra_fields() {
        let mut results = MergedResults::new();

        results.extra_fields.insert(
            "global".to_string(),
            serde_json::json!({"measurements": {"maxCustomMeasurements": 10}}),
        );
        results
            .extra_fields
            .insert("global_status".to_string(), serde_json::json!("ready"));

        let response = results.into_response().unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(
            parsed["global"]["measurements"]["maxCustomMeasurements"],
            10
        );
        assert_eq!(parsed["global_status"], "ready");
    }

    #[test]
    fn test_is_empty() {
        let results = MergedResults::new();

        // Empty with keys requested = empty
        assert!(results.is_empty(true));

        // Empty without keys requested = not empty (valid state)
        assert!(!results.is_empty(false));
    }
}
