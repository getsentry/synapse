use serde::Deserialize;
use std::collections::HashMap;

// TODO: This configuration is temporary: once these options are tested, we
// should choose the best one for use globally.
#[derive(Clone, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    None,
    Gzip,
    Zstd1,
    Zstd3,
}

#[derive(Clone, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum BackupRouteStoreType {
    Filesystem {
        base_dir: String,
        filename: String,
        compression: Compression,
    },
    Gcs {
        bucket: String,
        compression: Compression,
    },
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
#[serde(rename_all = "snake_case")]
pub enum LocatorDataType {
    Organization,
    ProjectKey,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(default)]
    pub listener: Listener,
    pub control_plane: ControlPlane,
    pub backup_route_store: BackupRouteStore,
    pub locality_to_default_cell: Option<HashMap<String, String>>,
    pub data_type: LocatorDataType,
}
