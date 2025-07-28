use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::SystemTime;

#[allow(dead_code)]
type HmacSha256 = Hmac<Sha256>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CellRecord {
    pub org_id: u64,
    pub org_name: String,
    pub cell: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CellRecords {
    pub records: Vec<CellRecord>,
}

#[allow(dead_code)]
pub struct FetchConfig {
    base_url: String,
    shared_secret: String,
    client: reqwest::Client,
}

#[allow(dead_code)]
impl FetchConfig {
    pub fn new(base_url: &str, shared_secret: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            shared_secret: shared_secret.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Generate HMAC signature for request body in format expected by server: "rpc0:{signature}"
    fn generate_signature(&self, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(self.shared_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(body);
        let signature = hex::encode(mac.finalize().into_bytes());
        format!("rpc0:{signature}")
    }

    /// Fetch all cell records. This should only be called during startup. For all other
    /// cases, should use the fetch_since method.
    pub async fn fetch_all(&self) -> Result<CellRecords> {
        let url = format!("{}/api/0/synapse-rpc/get_cell_records", self.base_url);

        // Create the request body with empty args: {"args": {}}
        let body_json = br#"{"args":{}}"#.to_vec();
        let signature = self.generate_signature(&body_json);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("rpcsignature {signature}"))
            .header("Content-Type", "application/json")
            .body(body_json)
            .send()
            .await
            .with_context(|| format!("Failed to fetch from {url}"))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "HTTP error: {} for URL: {}",
                response.status(),
                url
            ));
        }

        let cell_records = response
            .json::<CellRecords>()
            .await
            .context("Failed to parse response as JSON")?;

        Ok(cell_records)
    }

    /// Fetch incremental updates since timestamp
    pub async fn fetch_since(&self, since: SystemTime) -> Result<CellRecords> {
        let since_timestamp = since
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("Invalid timestamp")?
            .as_secs();

        let url = format!("{}/api/0/synapse-rpc/get_cell_records", self.base_url);

        // Create the request body with since timestamp in args: {"args": {"since": <timestamp>}}
        let body_json = format!(r#"{{"args":{{"since":{since_timestamp}}}}}"#).into_bytes();
        let signature = self.generate_signature(&body_json);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("rpcsignature {signature}"))
            .header("Content-Type", "application/json")
            .body(body_json)
            .send()
            .await
            .with_context(|| format!("Failed to fetch from {url}"))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "HTTP error: {} for URL: {}",
                response.status(),
                url
            ));
        }

        let cell_records = response
            .json::<CellRecords>()
            .await
            .context("Failed to parse response as JSON")?;

        Ok(cell_records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;
    use wiremock::matchers::{body_string, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // HTTP Integration tests using HMAC authentication
    #[tokio::test]
    async fn test_fetch_all_success() {
        let mock_server = MockServer::start().await;

        let response_body = r#"{
            "records": [
                {
                    "org_id": 12345,
                    "org_name": "some_org",
                    "cell": "us-1"
                },
                {
                    "org_id": 54321,
                    "org_name": "some_other_org",
                    "cell": "de-1"
                }
            ]
        }"#;

        // Expect POST request with empty args JSON body
        Mock::given(method("POST"))
            .and(path("/api/0/synapse-rpc/get_cell_records"))
            .and(body_string(r#"{"args":{}}"#))
            .respond_with(ResponseTemplate::new(200).set_body_string(response_body))
            .mount(&mock_server)
            .await;

        let fetcher = FetchConfig::new(&mock_server.uri(), "test-secret");
        let result = fetcher.fetch_all().await.unwrap();
        assert_eq!(result.records.len(), 2);
        assert_eq!(result.records[0].org_id, 12345);
        assert_eq!(result.records[0].org_name, "some_org");
        assert_eq!(result.records[1].org_id, 54321);
        assert_eq!(result.records[1].org_name, "some_other_org");
    }

    #[tokio::test]
    async fn test_fetch_since_with_timestamp() {
        let mock_server = MockServer::start().await;

        let timestamp = 1640995200u64;
        let response_body = r#"{
            "records": [
                {
                    "org_id": 999,
                    "org_name": "blah_blah",
                    "cell": "us-1"
                }
            ]
        }"#;

        // Expect POST request with timestamp in args JSON body
        let expected_body = format!(r#"{{"args":{{"since":{timestamp}}}}}"#);
        Mock::given(method("POST"))
            .and(path("/api/0/synapse-rpc/get_cell_records"))
            .and(body_string(expected_body))
            .respond_with(ResponseTemplate::new(200).set_body_string(response_body))
            .mount(&mock_server)
            .await;

        let fetcher = FetchConfig::new(&mock_server.uri(), "test-secret");
        let since_time = UNIX_EPOCH + std::time::Duration::from_secs(timestamp);
        let result = fetcher.fetch_since(since_time).await.unwrap();

        assert_eq!(result.records.len(), 1);
        assert_eq!(result.records[0].org_id, 999);
        assert_eq!(result.records[0].org_name, "blah_blah");
    }

    #[tokio::test]
    async fn test_http_error_handling() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/0/synapse-rpc/get_cell_records"))
            .and(body_string(r#"{"args":{}}"#))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let fetcher = FetchConfig::new(&mock_server.uri(), "test-secret");
        let result = fetcher.fetch_all().await;
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("404"));
    }
}
