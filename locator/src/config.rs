#![allow(dead_code)]

use serde::Deserialize;

#[derive(Debug, Deserialize)]
enum Adapter {
    None,
    File { path: String },
    Gcs { bucket: String },
}

#[derive(Debug, Deserialize)]
struct BackupRouteStore {
    r#type: Adapter,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    backup_route_store: Option<BackupRouteStore>,
}
