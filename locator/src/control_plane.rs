const BASE_DELAY: u64 = 500;

use crate::config::LocatorDataType;
use crate::metrics_defs::{CONTROL_PLANE_SYNC_DURATION, CONTROL_PLANE_SYNC_ROWS};
use crate::types::{CellId, RouteData};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Instant;
use tokio::time::{Duration, sleep};
use hmac::{Hmac, Mac};
use sha2::Sha256;

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
/// ```
/// Authorization: Signature synapse0:<base64-encoded-hmac-sha256-signature>
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
    hmac_secret: Option<String>,
}

impl ControlPlane {
    pub fn new(data_type: LocatorDataType, base_url: String) -> Self {
        let path = match data_type {
            LocatorDataType::Organization => "org-cell-mappings",
            LocatorDataType::ProjectKey => "projectkey-cell-mappings",
        };

        let full_url = format!("{}/{}/", base_url.trim_end_matches('/'), path);

        let hmac_secret = std::env::var("SYNAPSE_HMAC_SECRET")
        .ok()
        .or_else(|| {
            tracing::warn!("SYNAPSE_HMAC_SECRET not set, HMAC authentication disabled");
            None
        });

        ControlPlane {
            client: reqwest::Client::new(),
            full_url,
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

            let response = self.client.get(url).send().await?;

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
        );
        let response = control_plane.load_mappings(None).await;

        let mapping = response.unwrap().id_to_cell;

        assert_eq!(mapping.len(), 30);

        assert_eq!(mapping.get("sentry0").unwrap(), "us1");
    }
}
