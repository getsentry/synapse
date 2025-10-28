mod api;
pub mod backup_routes;
pub mod config;
mod control_plane;
pub mod locator;
mod types;
use std::sync::Arc;

use backup_routes::{
    BackupRouteProvider, FilesystemRouteProvider, GcsRouteProvider, NoopRouteProvider,
};
use config::BackupRouteStoreType;

/// Run the locator API in standalone mode.
pub async fn run(config: config::Config) -> Result<(), api::LocatorApiError> {
    let provider: Arc<dyn BackupRouteProvider + 'static> =
        get_provider(config.backup_route_store.r#type);

    api::serve(config.listener, config.control_plane, provider).await
}

pub fn get_provider(store_type: BackupRouteStoreType) -> Arc<dyn BackupRouteProvider + 'static> {
    match store_type {
        BackupRouteStoreType::None => Arc::new(NoopRouteProvider {}),
        BackupRouteStoreType::Filesystem { base_dir, filename } => {
            Arc::new(FilesystemRouteProvider::new(&base_dir, &filename))
        }
        BackupRouteStoreType::Gcs { bucket } => Arc::new(GcsRouteProvider::new(&bucket)),
    }
}
