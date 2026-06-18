use crate::auth::{RelayInfo, RelaySigner, RelayVerifier, generate_credentials_json};
use locator::backup_routes::{BackupRouteProvider, FilesystemRouteProvider};
use locator::client::Locator;
use locator::config::Compression;
use locator::types::RouteData;
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

pub async fn get_mock_provider() -> (tempfile::TempDir, FilesystemRouteProvider) {
    let route_data = RouteData::from(
        HashMap::from([
            ("a".repeat(32).into(), "us1".into()),
            ("b".repeat(32).into(), "us1".into()),
            ("c".repeat(32).into(), "de".into()),
        ]),
        Some("cursor1".into()),
        HashMap::from([("us1".into(), "us".into()), ("de".into(), "de".into())]),
    );

    let dir = tempfile::tempdir().unwrap();
    let provider = FilesystemRouteProvider::new(
        dir.path().to_str().unwrap(),
        "backup.bin",
        Compression::Zstd1,
    );
    provider.store(&route_data).await.unwrap();
    (dir, provider)
}

/// A signer plus a verifier that trusts that signer's freshly generated credentials, so signed
/// requests verify end-to-end.
pub fn make_signing_keypair() -> (RelaySigner, RelayVerifier) {
    let json = generate_credentials_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let id = parsed["id"].as_str().unwrap().to_string();
    let public_key = parsed["public_key"].as_str().unwrap().to_string();

    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(json.as_bytes()).unwrap();
    let signer = RelaySigner::from_file(tmp.path()).unwrap();

    let verifier =
        RelayVerifier::from_relays(HashMap::from([(id, RelayInfo { public_key })])).unwrap();

    (signer, verifier)
}

// Mock backup provider for testing
struct MockBackupProvider {
    data: RouteData,
}

#[async_trait::async_trait]
impl BackupRouteProvider for MockBackupProvider {
    async fn load(&self) -> Result<RouteData, locator::backup_routes::BackupError> {
        Ok(self.data.clone())
    }

    async fn store(&self, _data: &RouteData) -> Result<(), locator::backup_routes::BackupError> {
        Ok(())
    }
}

pub async fn create_test_locator(key_to_cell: HashMap<String, String>) -> Locator {
    let route_data = RouteData::from(
        key_to_cell,
        Some("cursor".to_string()),
        HashMap::from([
            ("us1".to_string(), "us".to_string()),
            ("us2".to_string(), "us".to_string()),
        ]),
    );

    let provider = Arc::new(MockBackupProvider { data: route_data });

    let service = locator::locator::Locator::new(
        locator::config::LocatorDataType::ProjectKey,
        "http://invalid-control-plane:8000".to_string(),
        provider,
        None,
        None,
    );

    let locator = Locator::from_in_process_service(service);

    // Wait for locator to be ready
    for _ in 0..50 {
        if locator.is_ready() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }
    assert!(locator.is_ready(), "Locator should be ready");

    locator
}
