use serde::Deserialize;

#[derive(Deserialize)]
enum Adapter {
    None,
    File { path: String },
    Gcs { bucket: String },
}

#[derive(Deserialize)]
struct BackupRoutes {
    r#type: Adapter,
}

#[derive(Deserialize)]
pub struct LocatorConfig {
    backup_routes: Option<BackupRoutes>
}