mod api;
mod backup_routes;
pub mod config;
mod cursor;
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

    api::serve(config.listener, provider).await
}

pub fn get_provider(store_type: BackupRouteStoreType) -> Arc<dyn BackupRouteProvider + 'static> {
    match store_type {
        BackupRouteStoreType::None => Arc::new(NoopRouteProvider {}),
        BackupRouteStoreType::Filesystem { path } => Arc::new(FilesystemRouteProvider::new(&path)),
        BackupRouteStoreType::Gcs { bucket } => Arc::new(GcsRouteProvider::new(&bucket)),
    }
}
