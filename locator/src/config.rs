use serde::Deserialize;

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "type")]
pub enum BackupRouteStoreType {
    None,
    Filesystem { base_dir: String, filename: String },
    Gcs { bucket: String },
}

#[derive(Deserialize, Debug)]
pub struct ControlPlane {
    pub url: String,
}

#[derive(Deserialize, Debug)]
pub struct BackupRouteStore {
    #[serde(flatten)]
    pub r#type: BackupRouteStoreType,
}

#[derive(Deserialize, Debug)]
pub struct Listener {
    pub host: String,
    pub port: u16,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub listener: Listener,
    pub control_plane: ControlPlane,
    pub backup_route_store: BackupRouteStore,
}
