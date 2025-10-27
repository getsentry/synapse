use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Config {
    pub proxy: ProxyConfig,
    pub logging: LoggingConfig,
    pub metrics: MetricsConfig,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ProxyConfig {
    pub listener: Listener,
    pub admin_listener: Listener,
    pub locale_to_cells: HashMap<String, Vec<String>>,
    pub upstreams: Vec<UpstreamConfig>,
    pub routes: Vec<Route>,
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
    pub action: Action,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Match {
    pub host: Option<String>,
    pub path: Option<String>,
    pub method: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Action {
    pub resolver: String,
    pub locale: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct LoggingConfig {
    pub sentry_dsn: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct MetricsConfig {
    pub statsd_host: String,
    pub statsd_port: u16,
}
