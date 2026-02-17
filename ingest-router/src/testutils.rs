use locator::backup_routes::{BackupRouteProvider, FilesystemRouteProvider};
use locator::client::Locator;
use locator::config::Compression;
use locator::types::RouteData;
use std::collections::HashMap;
use std::sync::Arc;

pub async fn get_mock_provider() -> (tempfile::TempDir, FilesystemRouteProvider) {
    let route_data = RouteData::from(
        HashMap::from([
            ("a".repeat(32).into(), "us1".into()),
            ("b".repeat(32).into(), "us1".into()),
            ("c".repeat(32).into(), "de".into()),
        ]),
        "cursor1".into(),
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
