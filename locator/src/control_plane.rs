#![allow(dead_code)]

const BASE_DELAY: u64 = 500;

use crate::types::{Cell, CellId, RouteData};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
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

// A cursor is passed for incremental loading. No cursor means the full snapshot will be loaded.
pub async fn load_mappings(
    base_url: &str,
    cursor: Option<&str>,
) -> Result<RouteData, ControlPlaneError> {
    const RETRIABLE_STATUS_CODES: &[StatusCode] = &[
        StatusCode::TOO_MANY_REQUESTS,     // 429
        StatusCode::INTERNAL_SERVER_ERROR, // 500
        StatusCode::BAD_GATEWAY,           // 502
        StatusCode::SERVICE_UNAVAILABLE,   // 503
        StatusCode::GATEWAY_TIMEOUT,       // 504
    ];

    let mut cells: HashMap<String, Arc<Cell>> = HashMap::new();
    let mut org_to_cell = HashMap::new();
    let mut next_cursor: Option<String> = cursor.map(String::from);
    let mut page_fetches = 0;
    // TODO: consider reusing client
    let client = reqwest::Client::new();

    // 3 retries per page fetch
    let mut retries = 0;

    loop {
        let mut url =
            Url::parse(base_url).map_err(|e| ControlPlaneError::InvalidUrl(e.to_string()))?;

        if let Some(ref c) = next_cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }

        let response = client.get(url).send().await?;

        if !response.status().is_success() {
            if RETRIABLE_STATUS_CODES.contains(&response.status()) && retries < 3 {
                retries += 1;
                // Backoff between retries
                let retry_millis = BASE_DELAY * 2_u64.pow(retries);
                sleep(Duration::from_millis(retry_millis)).await;
                continue;
            } else {
                return Err(ControlPlaneError::ControlPlaneRetriesExceeded);
            }
        }

        // Response successful, reset retries counter
        retries = 0;

        let json_response = response.json::<ControlPlaneData>().await?;

        for (c, l) in json_response.metadata.cell_to_locality {
            cells.insert(c.clone(), Arc::new(Cell::new(c, l)));
        }

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

    println!("Fetched {} pages from control plane", page_fetches);

    let data = RouteData {
        org_to_cell,
        // TODO: implement default cells, empty for now
        locality_to_default_cell: HashMap::new(),
        last_cursor: next_cursor.unwrap(),
        cells,
    };

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::process::{Child, Command};
    use std::time::Duration;

    struct TestServer {
        child: Child,
    }

    impl TestServer {
        fn spawn() -> std::io::Result<Self> {
            let child = Command::new("python")
                .arg("../scripts/mock_control_api.py")
                .spawn()?;

            Ok(Self { child })
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[tokio::test]
    async fn test_control_plane() {
        let _server = TestServer::spawn().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let control_plane_url = "http://127.0.0.1:9000/org-cell-mappings";
        let response = load_mappings(control_plane_url, None).await;

        let mapping = response.unwrap().org_to_cell;

        assert_eq!(mapping.len(), 30);

        assert_eq!(mapping.get("sentry0").unwrap(), "us1");
    }
}
