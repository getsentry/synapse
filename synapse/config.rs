
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

struct FanOutRouterConfig {}

struct ProxyConfig {}

struct LocatorConfig {}


struct Config {
    #[serde(flatten)]
    common: CommonConfig,
    fan_out_router: Option<FanOutRouterConfig>,
    proxy: Option<ProxyConfig>,
    locator: Option<LocatorConfig>,
}