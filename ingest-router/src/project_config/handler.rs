//! Handler implementation for the Relay Project Configs endpoint

use crate::api::utils::{deserialize_body, normalize_headers, serialize_to_body};
use crate::errors::IngestRouterError;
use crate::handler::{CellId, Handler, HandlerBody, SplitMetadata};
use crate::locale::Cells;
use crate::project_config::protocol::{ProjectConfigsRequest, ProjectConfigsResponse};
use async_trait::async_trait;
use http::StatusCode;
use http::response::Parts;
use hyper::header::{CONTENT_TYPE, HeaderValue};
use hyper::{Request, Response};
use locator::client::Locator;
use shared::http::make_error_response;
use std::collections::{HashMap, HashSet};

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

        let cell_ids: HashSet<&String> = cells.cell_list().iter().collect();

        // Route each public key to its owning cell using the locator service
        let mut cell_to_keys: HashMap<CellId, Vec<String>> = HashMap::new();
        let mut pending: Vec<String> = Vec::new();

        for public_key in public_keys {
            // TODO: Enforce locality here?
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
        responses: Vec<(CellId, Result<Response<HandlerBody>, IngestRouterError>)>,
        metadata: SplitMetadata,
    ) -> Response<HandlerBody> {
        // TODO: Consider refactoring to avoid runtime downcast
        let meta = metadata
            .downcast::<ProjectConfigsMetadata>()
            .unwrap_or(Box::new(ProjectConfigsMetadata::default()));

        let mut merged = ProjectConfigsResponse::new();
        merged.pending_keys.extend(meta.unassigned_keys);

        // Parts is populated from the first response. The responses are previously
        // sorted so successful responses (if they exist) come first.
        let mut parts: Option<Parts> = None;

        for (cell_id, result) in responses {
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

            if let Ok(parsed) = deserialize_body::<ProjectConfigsResponse>(body).await {
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

        if let (Some(mut p), Ok(body)) = (parts, serialized_body) {
            normalize_headers(&mut p.headers, p.version);
            p.headers
                .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

            Response::from_parts(p, body)
        } else {
            make_error_response(StatusCode::INTERNAL_SERVER_ERROR)
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

    fn build_request(project_configs_request: ProjectConfigsRequest) -> Request<HandlerBody> {
        let body = serialize_to_body(&project_configs_request).unwrap();
        Request::builder()
            .method("POST")
            .uri("/api/0/relays/projectconfigs/")
            .body(body)
            .unwrap()
    }

    fn build_response(project_configs_response: serde_json::Value) -> Response<HandlerBody> {
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

        let us1_body: ProjectConfigsRequest = deserialize_body(us1_req.into_body()).await.unwrap();

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

        let us2_body: ProjectConfigsRequest = deserialize_body(us2_req.into_body()).await.unwrap();

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

        let parsed: ProjectConfigsResponse = deserialize_body(merged.into_body()).await.unwrap();

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

        let results: Vec<(CellId, Result<Response<HandlerBody>, IngestRouterError>)> = vec![
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
        let parsed: ProjectConfigsResponse = deserialize_body(merged.into_body()).await.unwrap();

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
