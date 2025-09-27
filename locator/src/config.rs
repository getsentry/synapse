#![allow(dead_code)]

use serde::Deserialize;

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "type")]
pub enum BackupRouteStoreType {
    None,
    Filesystem { path: String },
    Gcs { bucket: String },
    // Temporary, for testing
    Placeholder,
}

#[derive(Deserialize)]
pub struct ControlPlane {
    pub url: String,
}

#[derive(Deserialize)]
pub struct BackupRouteStore {
    #[serde(flatten)]
    pub r#type: BackupRouteStoreType,
}

#[derive(Deserialize)]
pub struct Listener {
    pub host: String,
    pub port: u16,
}

#[derive(Deserialize)]
pub struct Config {
    pub listener: Listener,
    pub control_plane: ControlPlane,
    pub backup_route_store: BackupRouteStore,
}
