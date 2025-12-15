use locator::backup_routes::{BackupRouteProvider, FilesystemRouteProvider};
use locator::config::Compression;
use locator::types::RouteData;
use std::collections::HashMap;

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
