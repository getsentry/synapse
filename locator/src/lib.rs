mod api;
pub mod backup_routes;
pub mod config;
mod control_plane;
mod cursor;
pub mod locator;
mod negative_cache;
pub mod types;
use std::sync::Arc;

#[cfg(test)]
mod testutils;

use backup_routes::{BackupError, BackupRouteProvider, FilesystemRouteProvider, GcsRouteProvider};
use config::BackupRouteStoreType;

/// Run the locator API in standalone mode.
pub async fn run(config: config::Config) -> Result<(), api::LocatorApiError> {
    let provider = get_provider(config.backup_route_store.r#type).await?;

    api::serve(
        config.data_type,
        config.listener,
        config.control_plane,
        provider,
        config.locality_to_default_cell,
    )
    .await
}

pub async fn get_provider(
    store_type: BackupRouteStoreType,
) -> Result<Arc<dyn BackupRouteProvider + 'static>, BackupError> {
    match store_type {
        BackupRouteStoreType::Filesystem { base_dir, filename } => {
            Ok(Arc::new(FilesystemRouteProvider::new(&base_dir, &filename)))
        }
        BackupRouteStoreType::Gcs { bucket } => Ok(Arc::new(GcsRouteProvider::new(bucket).await?)),
    }
}
