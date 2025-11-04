const BASE_DELAY: u64 = 500;

use crate::types::{CellId, RouteData};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use std::collections::HashMap;
use tokio::time::{Duration, sleep};

#[derive(Deserialize)]
struct ControlPlaneRecord {
    id: String,
    slug: String,
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
}

pub struct ControlPlane {
    client: reqwest::Client,
    full_url: String,
}

impl ControlPlane {
    pub fn new(base_url: String) -> Self {
        let full_url = format!(
            "{}/{}/",
            base_url.trim_end_matches('/'),
            "org-cell-mappings"
        );

        ControlPlane {
            client: reqwest::Client::new(),
            full_url,
        }
    }

    // A cursor is passed for incremental loading. No cursor means the full snapshot will be loaded.
    pub async fn load_mappings(
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
                org_to_cell.insert(row.slug, row.cell);
            }

            page_fetches += 1;
            next_cursor = Some(json_response.metadata.cursor);

            if !json_response.metadata.has_more {
                break;
            }
        }

        println!("Fetched {page_fetches} pages from control plane");

        let data = RouteData::from(org_to_cell, next_cursor.unwrap(), cell_to_locality);

        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testutils::TestControlPlaneServer;
    use std::time::Duration;

    #[tokio::test]
    async fn test_control_plane() {
        let _server = TestControlPlaneServer::spawn("127.0.0.1", 9000).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let control_plane = ControlPlane::new("http://127.0.0.1:9000/".to_string());
        let response = control_plane.load_mappings(None).await;

        let mapping = response.unwrap().org_to_cell;

        assert_eq!(mapping.len(), 30);

        assert_eq!(mapping.get("sentry0").unwrap(), "us1");
    }
}
