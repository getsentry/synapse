#![allow(dead_code)]

use locator::config::LocatorConfig;
use serde::Deserialize;

#[derive(Deserialize)]

struct MetricsConfig {
    statsd_host: String,
    statsd_port: u16,
}

#[derive(Deserialize)]
struct LoggingConfig {
    sentry_dsn: String,
}

#[derive(Deserialize)]
struct CommonConfig {
    metrics: Option<MetricsConfig>,
    logging: Option<LoggingConfig>,
}

#[derive(Deserialize)]

struct IngestRouterConfig {}

#[derive(Deserialize)]
struct ProxyConfig {}

#[derive(Deserialize)]
pub struct Config {
    #[serde(flatten)]
    common: CommonConfig,
    ingest_router: Option<IngestRouterConfig>,
    proxy: Option<ProxyConfig>,
    locator: Option<LocatorConfig>,
}

impl Config {
    pub fn from_file(_path: &std::path::Path) -> Result<Self, ConfigError> {
        unimplemented!();
    }
}

#[derive(Debug)]
pub enum ConfigError {
    IoError(std::io::Error),
}
