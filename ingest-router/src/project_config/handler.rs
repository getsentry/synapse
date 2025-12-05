//! Handler implementation for the Relay Project Configs endpoint

use crate::handler::{CellId, Handler};
use crate::errors::IngestRouterError;
use crate::locale::Cells;
use crate::project_config::protocol::{ProjectConfigsRequest, ProjectConfigsResponse};
use async_trait::async_trait;
use locator::client::Locator;
use std::collections::HashMap;

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
impl Handler<ProjectConfigsRequest, ProjectConfigsResponse> for ProjectConfigsHandler {
    /// Pending public keys that couldn't be routed to any cell
    type SplitMetadata = Vec<String>;

    async fn split_requests(
        &self,
        request: ProjectConfigsRequest,
        _cells: &Cells,
    ) -> Result<(Vec<(CellId, ProjectConfigsRequest)>, Vec<String>), IngestRouterError> {
        let public_keys = request.public_keys;
        let extra_fields = request.extra_fields;

        // Route each public key to its owning cell using the locator service
        let mut split: HashMap<CellId, Vec<String>> = HashMap::new();
        let mut pending: Vec<String> = Vec::new();

        for public_key in public_keys {
            match self.locator.lookup(&public_key, None).await {
                Ok(cell_id) => {
                    split.entry(cell_id).or_default().push(public_key);
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

        // Build per-cell requests
        let cell_requests: Vec<(CellId, ProjectConfigsRequest)> = split
            .into_iter()
            .map(|(cell_id, keys)| {
                (
                    cell_id,
                    ProjectConfigsRequest {
                        public_keys: keys,
                        extra_fields: extra_fields.clone(),
                    },
                )
            })
            .collect();

        Ok((cell_requests, pending))
    }

    fn merge_results(
        &self,
        results: Vec<Result<(CellId, ProjectConfigsResponse), (CellId, IngestRouterError)>>,
        pending_from_split: Vec<String>,
    ) -> ProjectConfigsResponse {
        let mut merged = ProjectConfigsResponse::new();

        // Add pending keys from split phase
        if !pending_from_split.is_empty() {
            merged.pending_keys.get_or_insert_with(Vec::new).extend(pending_from_split);
        }

        // Results are provided pre-sorted by cell priority (highest first)
        // The executor ensures results are ordered so we can use the first successful response
        // for extra_fields and headers.
        // Failed cells are handled by the executor adding their keys to pending_from_split.
        let mut found_priority_cell = false;

        // Process successful results from each cell (already in priority order)
        for result in results.into_iter().flatten() {
            let (_cell_id, response) = result;
            
            if !response.project_configs.is_empty() {
                merged.project_configs.extend(response.project_configs.clone());
            }

            // Use extra_fields and headers from first successful cell (highest priority)
            if !found_priority_cell {
                if !response.extra_fields.is_empty() {
                    merged.extra_fields.extend(response.extra_fields.clone());
                }
                
                // Store headers from highest priority cell
                if !response.http_headers.is_empty() {
                    merged.http_headers = response.http_headers.clone();
                }
                
                found_priority_cell = true;
            }

            // Add any pending keys from upstream response
            if let Some(pending_keys) = response.pending_keys.as_ref().filter(|keys| !keys.is_empty()) {
                merged.pending_keys.get_or_insert_with(Vec::new).extend(pending_keys.clone());
            }
        }

        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;
    use crate::locale::Locales;
    use locator::backup_routes::BackupRouteProvider;
    use locator::types::RouteData;
    use std::collections::HashMap;
    use std::sync::Arc;
    use url::Url;

    // Mock backup provider for testing
    struct MockBackupProvider {
        data: RouteData,
    }

    #[async_trait::async_trait]
    impl BackupRouteProvider for MockBackupProvider {
        async fn load(&self) -> Result<RouteData, locator::backup_routes::BackupError> {
            Ok(self.data.clone())
        }

        async fn store(
            &self,
            _data: &RouteData,
        ) -> Result<(), locator::backup_routes::BackupError> {
            Ok(())
        }
    }

    fn create_test_locator(key_to_cell: HashMap<String, String>) -> Locator {
        let route_data = RouteData::from(
            key_to_cell,
            "cursor".to_string(),
            HashMap::from([
                ("us1".to_string(), "us".to_string()),
                ("us2".to_string(), "us".to_string()),
            ]),
        );

        let provider = Arc::new(MockBackupProvider { data: route_data });

        let service = locator::locator::Locator::new(
            locator::config::LocatorDataType::ProjectKey,
            "http://invalid-control-plane:9000".to_string(),
            provider,
            None,
        );
        Locator::from_in_process_service(service)
    }

    #[tokio::test]
    async fn test_split_requests_multiple_cells() {
        let key_to_cell = HashMap::from([
            ("key1".to_string(), "us1".to_string()),
            ("key2".to_string(), "us2".to_string()),
            ("key3".to_string(), "us1".to_string()),
        ]);
        let locator = create_test_locator(key_to_cell);

        // Wait for locator to be ready
        for _ in 0..50 {
            if locator.is_ready() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
        assert!(locator.is_ready(), "Locator should be ready");

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

        let request = ProjectConfigsRequest {
            public_keys: vec!["key1".to_string(), "key2".to_string(), "key3".to_string()],
            extra_fields: extra.clone(),
        };

        let (cell_requests, pending) = handler.split_requests(request, cells).await.unwrap();

        // Should have 2 cell requests (us1 and us2)
        assert_eq!(cell_requests.len(), 2);
        assert_eq!(pending.len(), 0);

        // Find us1 and us2 requests
        let us1_req = cell_requests
            .iter()
            .find(|(id, _)| id == "us1")
            .map(|(_, req)| req)
            .unwrap();
        let us2_req = cell_requests
            .iter()
            .find(|(id, _)| id == "us2")
            .map(|(_, req)| req)
            .unwrap();

        // Verify us1 has key1 and key3
        assert_eq!(us1_req.public_keys.len(), 2);
        assert!(us1_req.public_keys.contains(&"key1".to_string()));
        assert!(us1_req.public_keys.contains(&"key3".to_string()));
        assert_eq!(us1_req.extra_fields, extra);

        // Verify us2 has key2
        assert_eq!(us2_req.public_keys.len(), 1);
        assert!(us2_req.public_keys.contains(&"key2".to_string()));
        assert_eq!(us2_req.extra_fields, extra);
    }

    #[tokio::test]
    async fn test_split_requests_unknown_key_goes_to_pending() {
        let key_to_cell = HashMap::from([("key1".to_string(), "us1".to_string())]);
        let locator = create_test_locator(key_to_cell);

        // Wait for locator to be ready
        for _ in 0..50 {
            if locator.is_ready() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
        assert!(locator.is_ready(), "Locator should be ready");

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

        let request = ProjectConfigsRequest {
            public_keys: vec!["key1".to_string(), "unknown_key".to_string()],
            extra_fields: HashMap::new(),
        };

        let (cell_requests, pending) = handler.split_requests(request, cells).await.unwrap();

        // Should have 1 cell request (us1 with key1)
        assert_eq!(cell_requests.len(), 1);
        assert_eq!(cell_requests[0].0, "us1");
        assert_eq!(cell_requests[0].1.public_keys, vec!["key1".to_string()]);

        // Unknown key should be in pending metadata
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0], "unknown_key");
    }

    #[tokio::test]
    async fn test_merge_results_successful_cells() {
        let handler = ProjectConfigsHandler::new(create_test_locator(HashMap::new()));

        // Create response from us1 with key1 and global config
        let response1_json = serde_json::json!({
            "configs": {
                "key1": {"slug": "project1"}
            },
            "global": {"version": 1}
        });
        let response1 = serde_json::from_value(response1_json).unwrap();

        // Create response from us2 with key2 and different global config
        let response2_json = serde_json::json!({
            "configs": {
                "key2": {"slug": "project2"}
            },
            "global": {"version": 2}
        });
        let response2 = serde_json::from_value(response2_json).unwrap();

        let results = vec![
            Ok(("us1".to_string(), response1)),
            Ok(("us2".to_string(), response2)),
        ];

        let merged = handler.merge_results(results, vec![]);

        // Should have configs from both cells
        let json = serde_json::to_value(&merged).unwrap();
        assert_eq!(json["configs"].as_object().unwrap().len(), 2);
        assert!(json["configs"].get("key1").is_some());
        assert!(json["configs"].get("key2").is_some());

        // Should use global from first result (executor ensures proper ordering)
        assert_eq!(json["global"]["version"], 1);
    }

    #[tokio::test]
    async fn test_merge_results_with_pending() {
        let handler = ProjectConfigsHandler::new(create_test_locator(HashMap::new()));

        // Test all three sources of pending keys:
        // 1. From split phase (routing failures, unknown keys)
        // 2. From upstream response (async computation)
        // 3. From failed cells (added by executor)
        
        // Create response from us1 with successful config and upstream pending
        let response1_json = serde_json::json!({
            "configs": {
                "key1": {"slug": "project1"}
            },
            "pending": ["key_upstream_pending"]
        });
        let response1 = serde_json::from_value(response1_json).unwrap();

        // Create response from us2 with successful config
        let response2_json = serde_json::json!({
            "configs": {
                "key2": {"slug": "project2"}
            }
        });
        let response2 = serde_json::from_value(response2_json).unwrap();

        let results = vec![
            Ok(("us1".to_string(), response1)),
            Ok(("us2".to_string(), response2)),
        ];
        
        // Pending from split phase (routing failures) and failed cells (executor-added)
        let pending_from_split = vec![
            "key_routing_failed".to_string(),
            "key_from_failed_cell1".to_string(),
            "key_from_failed_cell2".to_string(),
        ];

        let merged = handler.merge_results(results, pending_from_split);

        let json = serde_json::to_value(&merged).unwrap();
        
        // Should have configs from both successful cells
        assert_eq!(json["configs"].as_object().unwrap().len(), 2);
        assert!(json["configs"].get("key1").is_some());
        assert!(json["configs"].get("key2").is_some());
        
        // Should have all pending keys from all three sources
        let pending = json["pending"].as_array().unwrap();
        assert_eq!(pending.len(), 4);
        assert!(pending.contains(&serde_json::json!("key_routing_failed")));
        assert!(pending.contains(&serde_json::json!("key_from_failed_cell1")));
        assert!(pending.contains(&serde_json::json!("key_from_failed_cell2")));
        assert!(pending.contains(&serde_json::json!("key_upstream_pending")));
    }
}

