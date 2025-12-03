//! Key routing logic for distributing requests across upstream cells.
//!
//! Uses the locator service to route public keys to their owning cells.

use crate::locale::Cells;
use locator::client::Locator;
use std::collections::HashMap;

#[allow(dead_code)]
pub type PublicKey = String;
#[allow(dead_code)]
pub type CellId = String;

/// Routes public keys to their owning cells using the locator service.
#[allow(dead_code)]
pub struct PublicKeySplitter {
    locator: Locator,
}

#[allow(dead_code)]
impl PublicKeySplitter {
    pub fn new(locator: Locator) -> Self {
        Self { locator }
    }

    /// Splits public keys to their owning cells using locator service lookups.
    ///
    /// For each public key, queries the locator to determine which cell owns it.
    /// Returns a map of cell IDs to their public keys, plus a list of keys that
    /// couldn't be routed (no cell found, locator errors, or unconfigured cells).
    pub async fn split(
        &self,
        public_keys: Vec<PublicKey>,
        cells: &Cells,
    ) -> (HashMap<CellId, Vec<PublicKey>>, Vec<PublicKey>) {
        if cells.cell_list.is_empty() {
            return (HashMap::new(), public_keys);
        }

        let mut split: HashMap<CellId, Vec<PublicKey>> = HashMap::new();
        let mut pending: Vec<PublicKey> = Vec::new();

        // Look up each public key to determine its owning cell
        for public_key in public_keys {
            match self.locator.lookup(&public_key, None).await {
                Ok(cell_id) => {
                    // Check if this cell is in our configured cells
                    if cells.cell_to_upstreams.contains_key(&cell_id) {
                        split.entry(cell_id).or_default().push(public_key);
                    } else {
                        // Cell not configured, add to pending
                        tracing::warn!(
                            public_key = %public_key,
                            cell_id = %cell_id,
                            "Public key routed to unconfigured cell"
                        );
                        pending.push(public_key);
                    }
                }
                Err(e) => {
                    // Locator errors, add to pending
                    tracing::info!(
                        public_key = %public_key,
                        error = ?e,
                        "Failed to route public key"
                    );
                    pending.push(public_key);
                }
            }
        }

        (split, pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;
    use crate::locale::Locales;
    use locator::backup_routes::BackupRouteProvider;
    use locator::types::RouteData;
    use std::collections::HashMap;
    use std::sync::Arc;
    use url::Url;

    // Mock backup provider for testing
    struct MockBackupProvider {
        data: RouteData,
    }

    #[async_trait::async_trait]
    impl BackupRouteProvider for MockBackupProvider {
        async fn load(&self) -> Result<RouteData, locator::backup_routes::BackupError> {
            Ok(self.data.clone())
        }

        async fn store(
            &self,
            _data: &RouteData,
        ) -> Result<(), locator::backup_routes::BackupError> {
            Ok(())
        }
    }

    fn create_test_locator(key_to_cell: HashMap<String, String>) -> Locator {
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
        );
        Locator::from_in_process_service(service)
    }

    #[tokio::test]
    async fn test_multiple_cells() {
        let key_to_cell = HashMap::from([
            ("key1".to_string(), "us1".to_string()),
            ("key2".to_string(), "us2".to_string()),
            ("key3".to_string(), "us1".to_string()),
        ]);
        let locator = create_test_locator(key_to_cell);

        // Wait for locator to be ready
        for _ in 0..50 {
            if locator.is_ready() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
        assert!(locator.is_ready(), "Locator should be ready");

        let splitter = PublicKeySplitter::new(locator);

        let locales = HashMap::from([(
            "us".to_string(),
            vec![
                CellConfig {
                    id: "us1".to_string(),
                    sentry_url: Url::parse("http://us1:8080").unwrap(),
                    relay_url: Url::parse("http://us1:8090").unwrap(),
                },
                CellConfig {
                    id: "us2".to_string(),
                    sentry_url: Url::parse("http://us2:8080").unwrap(),
                    relay_url: Url::parse("http://us2:8090").unwrap(),
                },
            ],
        )]);

        let locales_obj = Locales::new(locales);
        let cells = locales_obj.get_cells("us").unwrap();

        let public_keys = vec!["key1".to_string(), "key2".to_string(), "key3".to_string()];

        let (splits, pending) = splitter.split(public_keys, cells).await;

        assert_eq!(pending.len(), 0);
        assert_eq!(splits.len(), 2);

        // Verify us1 cell has key1 and key3
        let us1_keys = splits.get("us1").unwrap();
        assert_eq!(us1_keys.len(), 2);
        assert!(us1_keys.contains(&"key1".to_string()));
        assert!(us1_keys.contains(&"key3".to_string()));

        // Verify us2 cell has key2
        let us2_keys = splits.get("us2").unwrap();
        assert_eq!(us2_keys.len(), 1);
        assert!(us2_keys.contains(&"key2".to_string()));
    }

    #[tokio::test]
    async fn test_unknown_key_goes_to_pending() {
        let key_to_cell = HashMap::from([("key1".to_string(), "us1".to_string())]);
        let locator = create_test_locator(key_to_cell);

        // Wait for locator to be ready
        for _ in 0..50 {
            if locator.is_ready() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
        assert!(locator.is_ready(), "Locator should be ready");

        let splitter = PublicKeySplitter::new(locator);

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

        let public_keys = vec!["key1".to_string(), "unknown_key".to_string()];

        let (splits, pending) = splitter.split(public_keys, cells).await;

        assert_eq!(splits.len(), 1);
        assert!(splits.contains_key("us1"));
        assert_eq!(splits.get("us1").unwrap(), &vec!["key1".to_string()]);

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0], "unknown_key");
    }
}
