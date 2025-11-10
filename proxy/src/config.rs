use locator::config::{BackupRouteStore, ControlPlane};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Config {
    pub upstreams: Vec<UpstreamConfig>,
    pub routes: Vec<Route>,
    #[serde(default)]
    pub listener: Listener,
    #[serde(default)]
    pub admin_listener: AdminListener,
    pub locator: Locator,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Listener {
    pub host: String,
    pub port: u16,
}

impl Default for Listener {
    fn default() -> Self {
        Listener {
            host: "0.0.0.0".into(),
            port: 3000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct AdminListener {
    pub host: String,
    pub port: u16,
}

impl Default for AdminListener {
    fn default() -> Self {
        AdminListener {
            host: "0.0.0.0".into(),
            port: 3001,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct UpstreamConfig {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Route {
    pub r#match: Match,
    pub action: Action,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Match {
    pub host: Option<String>,
    pub path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Action {
    Dynamic {
        resolver: String,
        cell_to_upstream: HashMap<String, String>,
        default: Option<String>,
    },
    Static {
        to: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct HandlerConfig {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum LocatorType {
    #[serde(rename = "url")]
    Url { url: String },
    #[serde(rename = "in_process")]
    InProcess {
        control_plane: ControlPlane,
        backup_route_store: BackupRouteStore,
        locality_to_default_cell: Option<HashMap<String, String>>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Locator {
    #[serde(flatten)]
    pub r#type: LocatorType,
}
