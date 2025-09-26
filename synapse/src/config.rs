#![allow(dead_code)]

use locator::config::Config as LocatorConfig;
use serde::Deserialize;
use std::fs::File;

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
    pub locator: Option<LocatorConfig>,
}

impl Config {
    pub fn from_file(path: &std::path::Path) -> Result<Self, ConfigError> {
        let file = File::open(path)?;
        let data = serde_yaml::from_reader(file)?;

        Ok(data)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("could not load config from file: {0}")]
    LoadError(#[from] std::io::Error),
    #[error("could not parse config: {0}")]
    ParseError(#[from] serde_yaml::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use locator::config::BackupRouteStoreType;
    use std::io::Write;

    fn write_tmp_file(s: &str) -> tempfile::NamedTempFile {
        let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
        write!(tmp, "{}", s).expect("write yaml");

        tmp
    }

    #[test]
    fn locator_config() {
        let locator_yaml = r#"
            locator:
                listener:
                    host: 0.0.0.0
                    port: 8080
                control_plane:
                    url: control-plane.internal
                backup_route_store:
                    type: filesystem
                    path: /var/lib/locator/
            "#;
        let tmp = write_tmp_file(locator_yaml);
        let config = Config::from_file(tmp.path()).expect("load config");
        let locator_config = config.locator.expect("locator config");
        assert_eq!(locator_config.control_plane.url, "control-plane.internal");
        assert_eq!(
            locator_config.backup_route_store.r#type,
            BackupRouteStoreType::Filesystem {
                path: "/var/lib/locator/".into()
            }
        );
    }
}
