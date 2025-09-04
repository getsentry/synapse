
struct MetricsConfig {
    statsd_host: String,
    statsd_port: u16,
}

struct LoggingConfig {
    sentry_dsn: String,
}

struct CommonConfig {
    metrics: Option<MetricsConfig>,
    logging: Option<LoggingConfig>,
}

struct IngestRouterConfig {}

struct ProxyConfig {}

struct LocatorConfig {}


struct Config {
    #[serde(flatten)]
    common: CommonConfig,
    ingest_router: Option<IngestRouterConfig>,
    proxy: Option<ProxyConfig>,
    locator: Option<LocatorConfig>,
}