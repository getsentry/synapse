const BASE_DELAY: u64 = 500;
const AUTH_SCHEME: &str = "Signature";
const HMAC_SIGNATURE_PREFIX: &str = "synapse0";

use crate::config::LocatorDataType;
use crate::metrics_defs::{CONTROL_PLANE_SYNC_DURATION, CONTROL_PLANE_SYNC_ROWS};
use crate::types::{CellId, RouteData};
use hmac::{Hmac, Mac};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use sha2::Sha256;
use std::collections::HashMap;
use std::time::Instant;
use tokio::time::{Duration, sleep};

#[derive(Deserialize)]
struct ControlPlaneRecord {
    id: String,
    // Slug is present in organization but not project key responses
    slug: Option<String>,
    cell: CellId,
}

#[derive(Deserialize)]
struct ControlPlaneMetadata {
    cursor: String,
    has_more: bool,
    cell_to_locality: HashMap<String, String>,
}

#[derive(Deserialize)]
struct ControlPlaneData {
    data: Vec<ControlPlaneRecord>,
    metadata: ControlPlaneMetadata,
}

#[derive(thiserror::Error, Debug)]
pub enum ControlPlaneError {
    #[error("could not load config: {0}")]
    ReqwestError(#[from] reqwest::Error),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("control plane unavailable")]
    ControlPlaneRetriesExceeded,
    #[error("missing cursor in response")]
    MissingCursor,
}

/// Control plane client for syncing route mappings from Sentry's control silo.
///
/// # HMAC Authentication
///
/// The `SYNAPSE_HMAC_SECRET` environment variable should contain a raw string secret
/// that will be used as the key for HMAC-SHA256 authentication.
///
/// The signature will be included in HTTP requests to the control plane in the `Authorization` header:
///
/// ```text
/// Authorization: Signature synapse0:<hex-encoded-hmac-sha256-signature>
/// ```
///
/// The signature is computed as HMAC-SHA256 of path:body, signed with the secret
/// key from `SYNAPSE_HMAC_SECRET`. For GET requests (as used here), the body is empty bytes.
///
/// If `SYNAPSE_HMAC_SECRET` is not set, HMAC authentication will be disabled and a
/// warning will be logged. The `Authorization` header will not be added to requests.
pub struct ControlPlane {
    client: reqwest::Client,
    full_url: String,
    localities: Option<Vec<String>>,
    hmac_secret: Option<String>,
}

impl ControlPlane {
    pub fn new(data_type: LocatorDataType, base_url: String, localities: Option<Vec<String>>) -> Self {
        let path = match data_type {
            LocatorDataType::Organization => "api/0/internal/org-cell-mappings",
            LocatorDataType::ProjectKey => "api/0/internal/projectkey-cell-mappings",
        };

        let full_url = format!("{}/{}/", base_url.trim_end_matches('/'), path);

        let hmac_secret = std::env::var("SYNAPSE_HMAC_SECRET").ok().or_else(|| {
            tracing::warn!("SYNAPSE_HMAC_SECRET not set, HMAC authentication disabled");
            None
        });

        ControlPlane {
            client: reqwest::Client::new(),
            full_url,
            localities,
            hmac_secret,
        }
    }

    // A cursor is passed for incremental loading. No cursor means the full snapshot will be loaded.
    pub async fn load_mappings(
        &self,
        cursor: Option<&str>,
    ) -> Result<RouteData, ControlPlaneError> {
        let start = Instant::now();
        let sync_type = if cursor.is_some() {
            "incremental"
        } else {
            "snapshot"
        };

        let result = self.load_mappings_inner(cursor).await;

        let status = if result.is_ok() { "success" } else { "failure" };

        metrics::histogram!(CONTROL_PLANE_SYNC_DURATION.name, "type" => sync_type, "status" => status)
            .record(start.elapsed().as_secs_f64());

        if let Ok(ref data) = result {
            metrics::histogram!(CONTROL_PLANE_SYNC_ROWS.name, "type" => sync_type)
                .record(data.id_to_cell.len() as f64);
        }

        result
    }

    /// Computes HMAC-SHA256 signature for the given path and body.
    /// Returns the hex-encoded signature, using path:body format.
    fn compute_hmac_signature(secret: &str, path: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("HMAC can take key of any size");

        mac.update(path.as_bytes());
        mac.update(b":");
        mac.update(body);

        let result = mac.finalize();
        let code_bytes = result.into_bytes();
        code_bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

    async fn load_mappings_inner(
        &self,
        cursor: Option<&str>,
    ) -> Result<RouteData, ControlPlaneError> {
        const RETRIABLE_STATUS_CODES: &[StatusCode] = &[
            StatusCode::TOO_MANY_REQUESTS,     // 429
            StatusCode::INTERNAL_SERVER_ERROR, // 500
            StatusCode::BAD_GATEWAY,           // 502
            StatusCode::SERVICE_UNAVAILABLE,   // 503
            StatusCode::GATEWAY_TIMEOUT,       // 504
        ];

        let mut cell_to_locality: HashMap<String, String> = HashMap::new();
        let mut org_to_cell = HashMap::new();
        let mut next_cursor: Option<String> = cursor.map(String::from);
        let mut page_fetches = 0;

        // 3 retries per page fetch
        let mut retries = 0;

        loop {
            let mut url = Url::parse(&self.full_url)
                .map_err(|e| ControlPlaneError::InvalidUrl(e.to_string()))?;

            if let Some(ref c) = next_cursor {
                url.query_pairs_mut().append_pair("cursor", c);
            }

            // Add locality query parameters if configured
            if let Some(ref localities) = self.localities {
                for locality in localities {
                    url.query_pairs_mut().append_pair("locality", locality);
                }
            }

            // Build request with optional HMAC authentication
            let mut request = self.client.get(url.clone());

            if let Some(secret) = &self.hmac_secret {
                // For GET requests, body is empty bytes
                let signature = Self::compute_hmac_signature(secret, url.path(), &[]);
                let auth_header =
                    format!("{} {}:{}", AUTH_SCHEME, HMAC_SIGNATURE_PREFIX, signature);
                request = request.header("Authorization", auth_header);
            }

            let response = request.send().await?;

            if !response.status().is_success() {
                if RETRIABLE_STATUS_CODES.contains(&response.status()) && retries < 3 {
                    // Backoff between retries
                    let retry_millis = BASE_DELAY * 2_u64.pow(retries);
                    sleep(Duration::from_millis(retry_millis)).await;
                    retries += 1;
                    continue;
                } else {
                    return Err(ControlPlaneError::ControlPlaneRetriesExceeded);
                }
            }

            // Response successful, reset retries counter
            retries = 0;

            let json_response = response.json::<ControlPlaneData>().await?;

            cell_to_locality.extend(json_response.metadata.cell_to_locality);

            for row in json_response.data {
                org_to_cell.insert(row.id, row.cell.clone());
                if let Some(slug) = row.slug {
                    org_to_cell.insert(slug, row.cell);
                }
            }

            page_fetches += 1;
            next_cursor = Some(json_response.metadata.cursor);

            if !json_response.metadata.has_more {
                break;
            }
        }

        tracing::info!("Fetched {page_fetches} pages from control plane");

        let cursor = next_cursor.ok_or(ControlPlaneError::MissingCursor)?;
        let data = RouteData::from(org_to_cell, cursor, cell_to_locality);

        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testutils::TestControlPlaneServer;

    #[tokio::test]
    async fn test_control_plane() {
        let _server = TestControlPlaneServer::spawn("127.0.0.1", 9000).unwrap();
        let control_plane = ControlPlane::new(
            LocatorDataType::Organization,
            "http://127.0.0.1:9000/".to_string(),
            None,
        );
        let response = control_plane.load_mappings(None).await;

        let mapping = response.unwrap().id_to_cell;

        assert_eq!(mapping.len(), 30);

        assert_eq!(mapping.get("sentry0").unwrap(), "us1");
    }

    #[tokio::test]
    async fn test_control_plane_with_localities() {
        let _server = TestControlPlaneServer::spawn("127.0.0.1", 9002).unwrap();
        let control_plane = ControlPlane::new(
            LocatorDataType::Organization,
            "http://127.0.0.1:9002/".to_string(),
            Some(vec!["de".into()]),
        );
        let response = control_plane.load_mappings(None).await;

        let mapping = response.unwrap().id_to_cell;

        // Only the 3 "de" orgs (i=4,9,14) should be returned, each with id + slug = 6 entries
        assert_eq!(mapping.len(), 6);
        assert_eq!(mapping.get("4").unwrap(), "de1");
        assert_eq!(mapping.get("sentry4").unwrap(), "de1");
        assert_eq!(mapping.get("9").unwrap(), "de1");
        assert_eq!(mapping.get("14").unwrap(), "de1");
    }

    #[test]
    fn test_compute_hmac_signature() {
        let secret = "test_secret";
        let path = "/api/test";
        let body = b"";

        let signature = ControlPlane::compute_hmac_signature(secret, path, body);

        // Verify signature is 64 char hex string
        assert_eq!(signature.len(), 64);
        assert!(signature.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
