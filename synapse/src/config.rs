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
struct LoggingConfig {
    sentry_dsn: String,
}

#[derive(Debug, Deserialize)]
struct CommonConfig {
    metrics: Option<MetricsConfig>,
    logging: Option<LoggingConfig>,
}

#[derive(Debug, Deserialize)]
struct IngestRouterConfig {}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    common: CommonConfig,
    ingest_router: Option<IngestRouterConfig>,
    pub proxy: Option<ProxyConfig>,
    locator: Option<LocatorConfig>,
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
    use proxy::config::Listener;

    use super::*;
    use std::io::Write;

    fn write_tmp_file(s: &str) -> tempfile::NamedTempFile {
        let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
        write!(tmp, "{}", s).expect("write yaml");

        tmp
    }

    #[test]
    fn proxy_config() {
        let proxy_yaml = r#"
            proxy:
                upstreams: [{name: local, url: http://127.0.0.1:9000}]
                routes: [{match: {path: health}, handler: {name: health}}]
                listener:
                    host: 0.0.0.0
                    port: 8080
                locator:
                    type: in_process
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
                    path: Some("health".into()),
                },
                action: proxy::config::RouteAction::Handler {
                    handler: proxy::config::HandlerConfig {
                        name: "health".into()
                    },
                }
            }]
        );
    }
}
