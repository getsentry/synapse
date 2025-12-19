//! Handler implementation for the Relay Project Configs endpoint

use crate::api::utils::{deserialize_body, normalize_headers, serialize_to_body};
use crate::errors::IngestRouterError;
use crate::handler::{CellId, Handler, HandlerBody, SplitMetadata};
use crate::locale::Cells;
use crate::project_config::protocol::{ProjectConfigsRequest, ProjectConfigsResponse};
use async_trait::async_trait;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Request, Response};
use locator::client::Locator;
use std::collections::{HashMap, HashSet};

/// Pending public keys that couldn't be routed to any cell
type ProjectConfigsMetadata = Vec<String>;

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
    async fn split_request(
        &self,
        request: Request<HandlerBody>,
        cells: &Cells,
    ) -> Result<(Vec<(CellId, Request<HandlerBody>)>, SplitMetadata), IngestRouterError> {
        let (mut parts, body) = request.into_parts();
        let parsed: ProjectConfigsRequest = deserialize_body(body).await?;
        normalize_headers(&mut parts.headers, parts.version);

        let public_keys = parsed.public_keys;
        let extra_fields = parsed.extra_fields;

        let cell_ids: HashSet<&String> = cells.cell_list.iter().collect();

        // Route each public key to its owning cell using the locator service
        let mut split: HashMap<CellId, Vec<String>> = HashMap::new();
        let mut pending: Vec<String> = Vec::new();

        for public_key in public_keys {
            match self.locator.lookup(&public_key, None).await {
                Ok(cell_id) => {
                    if !cell_ids.contains(&cell_id) {
                        tracing::warn!(
                            public_key = %public_key,
                            cell_id = %cell_id,
                            "Located cell is not in the current locality's configured cells, adding to pending"
                        );
                        pending.push(public_key);
                        continue;
                    }

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

        let cell_requests = split
            .into_iter()
            .map(|(cell_id, keys)| {
                let project_configs_request = ProjectConfigsRequest {
                    public_keys: keys,
                    extra_fields: extra_fields.clone(),
                };

                let body = serialize_to_body(&project_configs_request)?;
                let req = Request::from_parts(parts.clone(), body);
                Ok((cell_id, req))
            })
            .collect::<Result<_, IngestRouterError>>()?;

        let metadata = Box::new(pending);
        Ok((cell_requests, metadata))
    }

    fn merge_responses(
        &self,
        responses: Vec<(CellId, Result<Response<HandlerBody>, IngestRouterError>)>,
        metadata: SplitMetadata,
    ) -> Response<HandlerBody> {
        // TODO: The current implementation does not handle errors from the results
        // parameter. The edge case to be handled are if any of the upstreams failed
        // to return a response for whatever reason. In scenarios like this, the
        // executor needs to provide all the project config keys which failed to
        // resolve on the upstream. We would need to add those project keys to the
        // pending response.

        let mut merged = ProjectConfigsResponse::new();

        // Downcast metadata to our specific type
        if let Ok(project_metadata) = metadata.downcast::<ProjectConfigsMetadata>() {
            merged.pending_keys.extend(*project_metadata);
        }
        // Filter to successful responses only
        let mut iter = responses
            .into_iter()
            .filter_map(|(cell_id, result)| result.ok().map(|r| (cell_id, r)));

        // Handle first successful result (highest priority)
        // Gets extra_fields, headers, configs, and pending
        // TODO: Actually parse the response body (requires async, so we'd need to
        // restructure or parse bodies before calling merge_responses)
        if let Some((_cell_id, _response)) = iter.next() {
            // For now, we can't parse the body here since this isn't async
            // The executor should parse response bodies before passing to merge
        }

        // Build the final response using into_response which handles serialization
        match merged.into_response() {
            Ok(response) => response,
            Err(e) => {
                tracing::error!(error = ?e, "Failed to build merged response");
                Response::builder()
                    .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Full::new(Bytes::from("{}")).map_err(|e| match e {}).boxed())
                    .unwrap()
            }
        }
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

    fn build_request(body: ProjectConfigsRequest) -> Request<HandlerBody> {
        let bytes = body.to_bytes().unwrap();
        Request::builder()
            .method("POST")
            .uri("/api/0/relays/projectconfigs/")
            .body(Full::new(bytes).map_err(|e| match e {}).boxed())
            .unwrap()
    }

    async fn parse_request_body(req: Request<HandlerBody>) -> ProjectConfigsRequest {
        let bytes = req.into_body().collect().await.unwrap().to_bytes();
        ProjectConfigsRequest::from_bytes(&bytes).unwrap()
    }

    #[tokio::test]
    async fn test_split_request_multiple_cells() {
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
        let us1_body = parse_request_body(us1_req).await;

        let key_to_cell = HashMap::from([
            ("key1".to_string(), "us1".to_string()),
            ("key2".to_string(), "us2".to_string()),
            ("key3".to_string(), "us1".to_string()),
        ]);
        let locator = create_test_locator(key_to_cell);
        for _ in 0..50 {
            if locator.is_ready() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
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
        let (cell_requests2, _) = handler.split_request(request2, &cells).await.unwrap();
        let (_, us2_req) = cell_requests2
            .into_iter()
            .find(|(id, _)| id == "us2")
            .unwrap();
        let us2_body = parse_request_body(us2_req).await;

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
    }

    #[tokio::test]
    async fn test_split_request_unknown_key_goes_to_pending() {
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

        let request = build_request(ProjectConfigsRequest {
            public_keys: vec!["key1".to_string(), "unknown_key".to_string()],
            extra_fields: HashMap::new(),
        });

        let (cell_requests, metadata) = handler.split_request(request, &cells).await.unwrap();

        // Should have 1 cell request (us1 with key1)
        assert_eq!(cell_requests.len(), 1);
        assert_eq!(cell_requests[0].0, "us1");
        let body = parse_request_body(cell_requests.into_iter().next().unwrap().1).await;
        assert_eq!(body.public_keys, vec!["key1".to_string()]);

        // Unknown key should be in pending metadata
        let pending = metadata.downcast::<ProjectConfigsMetadata>().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0], "unknown_key");
    }

    fn build_response(body: &serde_json::Value) -> Response<HandlerBody> {
        let bytes = Bytes::from(serde_json::to_vec(body).unwrap());
        Response::builder()
            .status(200)
            .body(Full::new(bytes).map_err(|e| match e {}).boxed())
            .unwrap()
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
        let response1 = build_response(&response1_json);

        // Create response from us2 with key2 and different global config
        let response2_json = serde_json::json!({
            "configs": {
                "key2": {"slug": "project2"}
            },
            "global": {"version": 2}
        });
        let response2 = build_response(&response2_json);

        let results: Vec<(CellId, Result<Response<HandlerBody>, IngestRouterError>)> = vec![
            ("us1".to_string(), Ok(response1)),
            ("us2".to_string(), Ok(response2)),
        ];

        let metadata: SplitMetadata = Box::new(Vec::<String>::new());
        let merged = handler.merge_responses(results, metadata);

        // The current implementation doesn't actually parse the response bodies
        // (noted as TODO in the code), so we just verify we get a valid response
        assert_eq!(merged.status(), 200);
    }

    #[tokio::test]
    async fn test_merge_responses_with_pending() {
        let handler = ProjectConfigsHandler::new(create_test_locator(HashMap::new()));

        // Test pending keys from split phase (routing failures, unknown keys)

        // Create response from us1 with successful config
        let response1_json = serde_json::json!({
            "configs": {
                "key1": {"slug": "project1"}
            }
        });
        let response1 = build_response(&response1_json);

        // Create response from us2 with successful config
        let response2_json = serde_json::json!({
            "configs": {
                "key2": {"slug": "project2"}
            }
        });
        let response2 = build_response(&response2_json);

        let results: Vec<(CellId, Result<Response<HandlerBody>, IngestRouterError>)> = vec![
            ("us1".to_string(), Ok(response1)),
            ("us2".to_string(), Ok(response2)),
        ];

        // Pending from split phase (routing failures)
        let pending_from_split: ProjectConfigsMetadata = vec![
            "key_routing_failed".to_string(),
            "key_from_failed_cell1".to_string(),
            "key_from_failed_cell2".to_string(),
        ];

        let metadata: SplitMetadata = Box::new(pending_from_split);
        let merged = handler.merge_responses(results, metadata);

        // Parse response body to check pending keys
        let bytes = merged.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        // Should have pending keys from split phase
        let pending = json["pending"].as_array().unwrap();
        assert_eq!(pending.len(), 3);
        assert!(pending.contains(&serde_json::json!("key_routing_failed")));
        assert!(pending.contains(&serde_json::json!("key_from_failed_cell1")));
        assert!(pending.contains(&serde_json::json!("key_from_failed_cell2")));
    }
}
