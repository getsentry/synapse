//! Protocol types for the Relay Project Configs endpoint (v3).
//!
//! This module defines the request and response structures for Sentry's
//! `/api/0/relays/projectconfigs/` endpoint.
//!
//! See the module-level documentation in `mod.rs` for complete protocol details.

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
