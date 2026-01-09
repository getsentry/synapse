use crate::api::utils::{deserialize_body, normalize_headers, serialize_to_body};
use crate::errors::IngestRouterError;
use crate::handler::{CellId, ExecutionMode, Handler, SplitMetadata};
use crate::locale::Cells;
use async_trait::async_trait;
use http::StatusCode;
use http::header::{CONTENT_TYPE, HeaderValue};
use http::response::Parts;
use hyper::body::Bytes;
use hyper::{Request, Response};
use serde::{Deserialize, Serialize};
use shared::http::make_error_response;
use std::collections::{HashMap, HashSet};

/// Handler for the public keys endpoint.
///
/// `POST /api/0/relays/publickeys/`
///
///
/// Example request:
/// ```json
/// {
///   "relay_ids": ["key1", "key2"]
/// }
/// ```
///
/// Example response:
/// (key2 is not found in the upstreams)
///
/// ```json
/// {
///   "public_keys": {
///     "key1": "abc123...",
///     "key2": null
///   },
///   "relays": {
///     "key1": {
///       "publicKey": "abc123...",
///       "internal": false
///     },
///     "key2": null
///   }
/// }
/// ```

#[derive(Serialize, Deserialize)]
struct RelayInfo {
    #[serde(rename = "publicKey")]
    public_key: String,
    internal: bool,
}

#[derive(Serialize, Deserialize)]
struct PublicKeysRequest {
    relay_ids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct PublicKeysResponse {
    // TODO: check casing of API response
    public_keys: HashMap<String, Option<String>>,
    relays: HashMap<String, Option<RelayInfo>>,
}

struct PublicKeysMetadata {
    requested_relay_ids: HashSet<String>,
}

pub struct PublicKeysHandler;

#[async_trait]
impl Handler for PublicKeysHandler {
    fn name(&self) -> &'static str {
        "PublicKeysHandler"
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }

    async fn split_request(
        &self,
        request: Request<Bytes>,
        cells: &Cells,
    ) -> Result<(Vec<(CellId, Request<Bytes>)>, SplitMetadata), IngestRouterError> {
        let (mut parts, body) = request.into_parts();
        normalize_headers(&mut parts.headers, parts.version);

        // Extract requested relay_ids from the request
        let request_data: PublicKeysRequest = deserialize_body(body.clone())?;
        let requested_relay_ids: HashSet<String> = request_data.relay_ids.into_iter().collect();

        // Send the request to all cells
        let cell_requests = cells
            .cell_list()
            .iter()
            .map(|cell_id| {
                let req = Request::from_parts(parts.clone(), body.clone());
                (cell_id.clone(), req)
            })
            .collect();

        let metadata = Box::new(PublicKeysMetadata {
            requested_relay_ids,
        });

        Ok((cell_requests, metadata))
    }

    /// Merges responses from all cells into a single response.
    ///
    /// Returns success if we have values (not None) for all requested relay_ids,
    /// or if all cels succeed.
    async fn merge_responses(
        &self,
        responses: Vec<(CellId, Result<Response<Bytes>, IngestRouterError>)>,
        metadata: SplitMetadata,
    ) -> Response<Bytes> {
        let meta = match metadata.downcast::<PublicKeysMetadata>() {
            Ok(meta) => meta,
            Err(_) => return make_error_response(StatusCode::INTERNAL_SERVER_ERROR),
        };

        let mut has_failed_responses = false;

        // Initialize all requested relay_ids with None values
        let mut public_keys: HashMap<String, Option<String>> = meta
            .requested_relay_ids
            .iter()
            .map(|id| (id.clone(), None))
            .collect();
        let mut relays: HashMap<String, Option<RelayInfo>> = meta
            .requested_relay_ids
            .iter()
            .map(|id| (id.clone(), None))
            .collect();

        // Parts is populated from the first successful response.
        let mut parts: Option<Parts> = None;

        // Process responses, tracking failures
        for (_cell_id, result) in responses {
            let response = match result {
                Ok(r) if r.status().is_success() => r,
                _ => {
                    has_failed_responses = true;
                    continue; // Skip failed responses
                }
            };
            let (p, body) = response.into_parts();
            if parts.is_none() {
                parts = Some(p);
            }
            let parsed: PublicKeysResponse = match deserialize_body(body) {
                Ok(p) => p,
                Err(_) => {
                    has_failed_responses = true;
                    continue; // Skip deserialization failures
                }
            };

            // Insert into public_keys map: only if key doesn't exist or value being inserted is Some
            for (key, value) in parsed.public_keys {
                if !public_keys.contains_key(&key) || value.is_some() {
                    public_keys.insert(key, value);
                }
            }

            // Insert into relays map: only if key doesn't exist or value being inserted is Some
            for (key, value) in parsed.relays {
                if !relays.contains_key(&key) || value.is_some() {
                    relays.insert(key, value);
                }
            }
        }

        // If there were failures, check if we have non-None values for all requested relay_ids
        // If there were no failures, return success even if some values are None.
        if has_failed_responses {
            let has_all_values = meta.requested_relay_ids.iter().all(|relay_id| {
                public_keys.get(relay_id).and_then(|v| v.as_ref()).is_some()
                    && relays.get(relay_id).and_then(|v| v.as_ref()).is_some()
            });

            if !has_all_values {
                return make_error_response(StatusCode::BAD_GATEWAY);
            }
        }

        let mut p = match parts {
            Some(p) => p,
            None => return make_error_response(StatusCode::BAD_GATEWAY),
        };

        let body = match serialize_to_body(&PublicKeysResponse {
            public_keys,
            relays,
        }) {
            Ok(b) => b,
            Err(_) => return make_error_response(StatusCode::INTERNAL_SERVER_ERROR),
        };

        normalize_headers(&mut p.headers, p.version);
        p.headers
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Response::from_parts(p, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;
    use crate::locale::Locales;
    use std::collections::HashMap;
    use url::Url;

    fn create_test_cells() -> Cells {
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
        Locales::new(locales).get_cells("us").unwrap()
    }

    #[tokio::test]
    async fn test_split_request_sends_to_all_cells() {
        let handler = PublicKeysHandler;
        let cells = create_test_cells();

        let request_body = serde_json::json!({
            "relay_ids": ["key1", "key2"]
        });
        let body_bytes = Bytes::from(serde_json::to_vec(&request_body).unwrap());

        let request = Request::builder()
            .method("POST")
            .uri("/api/0/relays/publickeys/")
            .body(body_bytes)
            .unwrap();

        let (cell_requests, metadata) = handler.split_request(request, &cells).await.unwrap();

        assert_eq!(cell_requests.len(), 2);
        let cell_ids: Vec<_> = cell_requests.iter().map(|(id, _)| id.as_str()).collect();
        assert!(cell_ids.contains(&"us1"));
        assert!(cell_ids.contains(&"us2"));

        // Verify metadata contains requested relay_ids
        let meta = metadata.downcast::<PublicKeysMetadata>().unwrap();
        assert_eq!(meta.requested_relay_ids.len(), 2);
        assert!(meta.requested_relay_ids.contains("key1"));
        assert!(meta.requested_relay_ids.contains("key2"));
    }

    #[tokio::test]
    async fn test_merge_responses_all_success() {
        let handler = PublicKeysHandler;

        let response_body = serde_json::json!({
            "public_keys": {
                "key1": "pubkey1",
                "key2": "pubkey2"
            },
            "relays": {
                "key1": {
                    "publicKey": "pubkey1",
                    "internal": false
                },
                "key2": {
                    "publicKey": "pubkey2",
                    "internal": true
                }
            }
        });
        let body_bytes = Bytes::from(serde_json::to_vec(&response_body).unwrap());

        let success_response = Response::builder()
            .status(StatusCode::OK)
            .body(body_bytes)
            .unwrap();

        let metadata = Box::new(PublicKeysMetadata {
            requested_relay_ids: HashSet::from(["key1".to_string(), "key2".to_string()]),
        });

        let body_bytes2 = Bytes::from(serde_json::to_vec(&response_body).unwrap());
        let success_response2 = Response::builder()
            .status(StatusCode::OK)
            .body(body_bytes2)
            .unwrap();

        let responses = vec![
            ("us1".to_string(), Ok(success_response)),
            ("us2".to_string(), Ok(success_response2)),
        ];

        let merged = handler.merge_responses(responses, metadata).await;

        assert_eq!(merged.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_merge_responses_with_failures_but_all_values() {
        let handler = PublicKeysHandler;

        let response_body = serde_json::json!({
            "public_keys": {
                "key1": "pubkey1",
                "key2": "pubkey2"
            },
            "relays": {
                "key1": {
                    "publicKey": "pubkey1",
                    "internal": false
                },
                "key2": {
                    "publicKey": "pubkey2",
                    "internal": true
                }
            }
        });
        let body_bytes = Bytes::from(serde_json::to_vec(&response_body).unwrap());

        let success_response = Response::builder()
            .status(StatusCode::OK)
            .body(body_bytes)
            .unwrap();

        let metadata = Box::new(PublicKeysMetadata {
            requested_relay_ids: HashSet::from(["key1".to_string(), "key2".to_string()]),
        });

        let responses = vec![
            (
                "us1".to_string(),
                Err(IngestRouterError::UpstreamTimeout("us1".to_string())),
            ),
            ("us2".to_string(), Ok(success_response)),
        ];

        let merged = handler.merge_responses(responses, metadata).await;

        // Should succeed because we have all values despite one failure
        assert_eq!(merged.status(), StatusCode::OK);
    }
}
