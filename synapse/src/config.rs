#![allow(dead_code)]

use ingest_router::config::Config as IngestRouterConfig;
use locator::config::Config as LocatorConfig;
use proxy::config::Config as ProxyConfig;
use serde::Deserialize;
use std::fs::File;

#[derive(Debug, Deserialize)]
struct MetricsConfig {
    statsd_host: String,
    statsd_port: u16,
}

#[derive(Debug, Deserialize)]
pub struct LoggingConfig {
    pub sentry_dsn: String,
}

#[derive(Debug, Deserialize)]
pub struct CommonConfig {
    metrics: Option<MetricsConfig>,
    pub logging: Option<LoggingConfig>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub common: CommonConfig,
    pub ingest_router: Option<IngestRouterConfig>,
    pub proxy: Option<ProxyConfig>,
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
    use proxy::config::Listener;
    use std::io::Write;
    use std::path::Path;

    fn write_tmp_file(s: &str) -> tempfile::NamedTempFile {
        let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
        write!(tmp, "{s}").expect("write yaml");

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
                    base_dir: /var/lib/locator/
                    filename: backup.bin
            "#;
        let tmp = write_tmp_file(locator_yaml);
        let config = Config::from_file(tmp.path()).expect("load config");
        let locator_config = config.locator.expect("locator config");
        assert_eq!(locator_config.control_plane.url, "control-plane.internal");
        assert_eq!(
            locator_config.backup_route_store.r#type,
            BackupRouteStoreType::Filesystem {
                base_dir: "/var/lib/locator/".into(),
                filename: "backup.bin".into()
            }
        );
    }

    #[test]
    fn proxy_config() {
        let proxy_yaml = r#"
            proxy:
                upstreams: [{name: local, url: http://127.0.0.1:9000}]
                routes: [{match: {path: test}, action: {to: local}}]
                listener:
                    host: 0.0.0.0
                    port: 8080
                admin_listener:
                    host: 0.0.0.0
                    port: 8081
                locator:
                    type: in_process
                    control_plane:
                        url: control-plane.internal
                    backup_route_store:
                        type: filesystem
                        base_dir: /var/lib/locator/
                        filename: backup.bin

            "#;
        let tmp = write_tmp_file(proxy_yaml);
        let config = Config::from_file(tmp.path()).expect("load config");
        let proxy_config = config.proxy.expect("proxy config");
        assert_eq!(
            &proxy_config.listener,
            &Listener {
                host: "0.0.0.0".into(),
                port: 8080
            }
        );
        assert_eq!(
            &proxy_config.routes,
            &vec![proxy::config::Route {
                r#match: proxy::config::Match {
                    host: None,
                    path: Some("test".into()),
                },
                action: proxy::config::Action::Static { to: "local".into() }
            }]
        );
    }

    #[test]
    fn test_parse_example_config_file() {
        let path = Path::new("../example_config_ingest_router.yaml");
        let config = Config::from_file(path).expect("load config");
        let ingest_router_config = config.ingest_router.expect("ingest router config");

        ingest_router_config
            .validate()
            .expect("Example config validation failed");

        assert_eq!(ingest_router_config.listener.port, 3000);
        assert_eq!(ingest_router_config.admin_listener.port, 3001);
        assert!(!ingest_router_config.upstreams.is_empty());
        assert!(!ingest_router_config.routes.is_empty());
    }
}
