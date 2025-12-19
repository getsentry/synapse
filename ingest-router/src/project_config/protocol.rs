//! Protocol types for the Relay Project Configs endpoint (v3).
//!
//! This module defines the request and response structures for Sentry's
//! `/api/0/relays/projectconfigs/` endpoint.
//!
//! See the module-level documentation in `mod.rs` for complete protocol details.

use crate::errors::IngestRouterError;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Bytes;
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderMap};
use hyper::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use shared::http::filter_hop_by_hop;
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

    /// HTTP headers from the highest priority upstream (not serialized).
    #[serde(skip)]
    pub http_headers: HeaderMap,
}

impl ProjectConfigsResponse {
    pub fn new() -> Self {
        Self {
            project_configs: HashMap::new(),
            pending_keys: Vec::new(),
            extra_fields: HashMap::new(),
            http_headers: HeaderMap::new(),
        }
    }

    /// Builds an HTTP response from the merged results.
    /// Filters out hop-by-hop headers and Content-Length (which is recalculated for the new body).
    pub fn into_response(
        mut self,
    ) -> Result<Response<BoxBody<Bytes, IngestRouterError>>, IngestRouterError> {
        let merged_json = serde_json::to_vec(&self)
            .map_err(|e| IngestRouterError::ResponseSerializationError(e.to_string()))?;

        filter_hop_by_hop(&mut self.http_headers, hyper::Version::HTTP_11);
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
            .map_err(|e| IngestRouterError::HyperError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use hyper::header::{CACHE_CONTROL, HeaderValue};

    #[test]
    fn test_response_serialization() {
        let mut configs = HashMap::new();
        configs.insert(
            "key1".to_string(),
            serde_json::json!({
                "disabled": false,
                "slug": "test-project"
            }),
        );

        let response = ProjectConfigsResponse {
            project_configs: configs,
            pending_keys: vec!["key2".to_string()],
            extra_fields: HashMap::new(),
            http_headers: HeaderMap::new(),
        };

        let bytes = Bytes::from(serde_json::to_vec(&response).unwrap());
        let parsed = ProjectConfigsResponse::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.project_configs.len(), 1);
        assert_eq!(parsed.pending_keys.len(), 1);
    }

    #[tokio::test]
    async fn test_response_merging() {
        let mut results = ProjectConfigsResponse::new();

        // Merge configs from multiple upstreams
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

        results.project_configs.extend(configs1);
        results.project_configs.extend(configs2);

        // Add pending keys
        results
            .pending_keys
            .extend(vec!["key3".to_string(), "key4".to_string()]);
        results.pending_keys.extend(vec!["key5".to_string()]);

        // Merge extra fields
        let mut extra = HashMap::new();
        extra.insert(
            "global".to_string(),
            serde_json::json!({"measurements": {"maxCustomMeasurements": 10}}),
        );
        extra.insert("global_status".to_string(), serde_json::json!("ready"));
        results.extra_fields.extend(extra);

        let response = results.into_response().unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

        // Verify configs merged correctly
        assert_eq!(parsed["configs"].as_object().unwrap().len(), 2);
        assert!(parsed["configs"].get("key1").is_some());
        assert!(parsed["configs"].get("key2").is_some());

        // Verify pending keys added correctly
        assert_eq!(parsed["pending"].as_array().unwrap().len(), 3);

        // Verify extra fields merged correctly
        assert_eq!(
            parsed["global"]["measurements"]["maxCustomMeasurements"],
            10
        );
        assert_eq!(parsed["global_status"], "ready");
    }

    #[tokio::test]
    async fn test_empty_pending_omitted() {
        let results = ProjectConfigsResponse::new();
        let response = results.into_response().unwrap();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

        assert!(parsed.get("pending").is_none());
    }

    #[tokio::test]
    async fn test_headers_forwarding() {
        let mut results = ProjectConfigsResponse::new();

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
}
