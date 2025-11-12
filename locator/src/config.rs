use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "type")]
pub enum BackupRouteStoreType {
    Filesystem { base_dir: String, filename: String },
    Gcs { bucket: String },
}

#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct ControlPlane {
    pub url: String,
}

#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct BackupRouteStore {
    #[serde(flatten)]
    pub r#type: BackupRouteStoreType,
}

#[derive(Deserialize, Debug)]
pub struct Listener {
    pub host: String,
    pub port: u16,
}

impl Default for Listener {
    fn default() -> Self {
        Listener {
            host: "127.0.0.1".into(),
            port: 3000,
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(default)]
    pub listener: Listener,
    pub control_plane: ControlPlane,
    pub backup_route_store: BackupRouteStore,
    pub locality_to_default_cell: Option<HashMap<String, String>>,
}
