#![allow(dead_code)]

use serde::Deserialize;

#[derive(Deserialize)]
enum Adapter {
    None,
    File { path: String },
    Gcs { bucket: String },
}

#[derive(Deserialize)]
struct BackupRouteStore {
    r#type: Adapter,
}

#[derive(Deserialize)]
pub struct LocatorConfig {
    backup_route_store: Option<BackupRouteStore>,
}
