//! Protocol types for the Relay Project Configs endpoint (v3).
//!
//! This module defines the request and response structures for Sentry's
//! `/api/0/relays/projectconfigs/` endpoint.
//!
//! See the module-level documentation in `mod.rs` for complete protocol details.

use hyper::body::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
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

impl ProjectConfigsRequest {
    pub fn from_bytes(bytes: &Bytes) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    pub fn to_bytes(&self) -> Result<Bytes, serde_json::Error> {
        let json = serde_json::to_vec(self)?;
        Ok(Bytes::from(json))
    }
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
    #[serde(rename = "pending", skip_serializing_if = "Option::is_none")]
    pub pending_keys: Option<Vec<String>>,

    /// Other fields (`global`, `global_status`, future fields).
    #[serde(flatten)]
    pub extra_fields: HashMap<String, JsonValue>,
}

impl ProjectConfigsResponse {
    pub fn from_bytes(bytes: &Bytes) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let mut extra = HashMap::new();
        extra.insert("global".to_string(), serde_json::json!(true));
        extra.insert("noCache".to_string(), serde_json::json!(false));

        let request = ProjectConfigsRequest {
            public_keys: vec!["key1".to_string(), "key2".to_string()],
            extra_fields: extra,
        };

        let bytes = request.to_bytes().unwrap();
        let parsed = ProjectConfigsRequest::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.public_keys.len(), 2);
        assert_eq!(
            parsed.extra_fields.get("global"),
            Some(&serde_json::json!(true))
        );
    }

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
            pending_keys: Some(vec!["key2".to_string()]),
            extra_fields: HashMap::new(),
        };

        let bytes = Bytes::from(serde_json::to_vec(&response).unwrap());
        let parsed = ProjectConfigsResponse::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.project_configs.len(), 1);
        assert!(parsed.pending_keys.is_some());
        assert_eq!(parsed.pending_keys.unwrap().len(), 1);
    }
}
