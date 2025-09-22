use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Config {
    pub upstreams: Vec<UpstreamConfig>,
    pub routes: Vec<Route>,
    pub listener: Listener,
    pub locator: Locator,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Listener {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct UpstreamConfig {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Route {
    pub r#match: Match,
    #[serde(flatten)]
    pub action: RouteAction,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Match {
    pub host: Option<String>,
    pub path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RouteAction {
    Proxy { proxy: ProxyConfig },
    Handler { handler: HandlerConfig },
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ProxyConfig {
    Dynamic {
        resolver: String,
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
    InProcess { backup_route_store: Option<String> },
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Locator {
    #[serde(flatten)]
    r#type: LocatorType,
}
