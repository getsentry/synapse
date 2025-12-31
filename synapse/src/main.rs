use clap::{Args, Parser};
use std::path::PathBuf;

mod config;
use config::{Config, MetricsConfig};
use metrics_exporter_statsd::StatsdBuilder;
use std::future::Future;
use std::process;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Parser)]
enum CliCommand {
    Locator(LocatorArgs),
    Proxy(ProxyArgs),
    IngestRouter(IngestRouterArgs),
    /// Show all metrics definitions as markdown table
    ShowMetrics,
    /// Sync METRICS.md with current metric definitions
    SyncMetrics,
}

#[derive(thiserror::Error, Debug)]
enum CliError {
    #[error("Failed to load config file: {0}")]
    ConfigLoadError(#[from] config::ConfigError),
    #[error("Invalid config: {0}")]
    InvalidConfig(&'static str),
    #[error("Failed to create runtime: {0}")]
    RuntimeError(#[from] std::io::Error),
}

fn main() {
    init_tracing();

    if let Err(e) = cli() {
        tracing::error!(error = %e, "Startup error");
        std::process::exit(1);
    }
}

fn cli() -> Result<(), CliError> {
    let cmd = CliCommand::parse();

    match &cmd {
        CliCommand::Locator(locator_args) => {
            let config = Config::from_file(&locator_args.base.config_file_path)?;
            let _sentry_guard = init_sentry(config.common.logging);
            init_statsd_recorder("synapse.locator", config.common.metrics);

            let locator_config = config
                .locator
                .ok_or(CliError::InvalidConfig("Missing locator config"))?;

            run_async(locator::run(locator_config))?;
            Ok(())
        }
        CliCommand::Proxy(proxy_args) => {
            let config = Config::from_file(&proxy_args.base.config_file_path)?;
            let _sentry_guard = init_sentry(config.common.logging);
            init_statsd_recorder("synapse.proxy", config.common.metrics);

            let proxy_config = config
                .proxy
                .ok_or(CliError::InvalidConfig("Missing proxy config"))?;

            run_async(proxy::run(proxy_config))?;

            Ok(())
        }
        CliCommand::IngestRouter(ingest_router_args) => {
            let config = Config::from_file(&ingest_router_args.base.config_file_path)?;
            let _sentry_guard = init_sentry(config.common.logging);
            init_statsd_recorder("synapse.ingest_router", config.common.metrics);

            let ingest_router_config = config
                .ingest_router
                .ok_or(CliError::InvalidConfig("Missing ingest-router config"))?;

            tracing::info!("Starting ingest-router with config {ingest_router_config:#?}");

            Ok(())
        }
        CliCommand::ShowMetrics => {
            println!(
                "{}",
                generate_metrics_table(locator::metrics_defs::ALL_METRICS)
            );
            Ok(())
        }
        CliCommand::SyncMetrics => {
            let path = "METRICS.md";
            let mut content = std::fs::read_to_string(path).expect("Failed to read METRICS.md");

            content = sync_section(
                &content,
                "LOCATOR_METRICS",
                &generate_metrics_table(locator::metrics_defs::ALL_METRICS),
            );

            std::fs::write(path, content).expect("Failed to write METRICS.md");
            println!("Synced METRICS.md");
            Ok(())
        }
    }
}

fn sync_section(content: &str, name: &str, table: &str) -> String {
    let start_marker = format!("<!-- {}:START -->", name);
    let end_marker = format!("<!-- {}:END -->", name);

    let start_idx = content
        .find(&start_marker)
        .unwrap_or_else(|| panic!("Missing {} marker", start_marker));
    let end_idx = content
        .find(&end_marker)
        .unwrap_or_else(|| panic!("Missing {} marker", end_marker));

    format!(
        "{}{}\n{}\n{}{}",
        &content[..start_idx],
        start_marker,
        table,
        end_marker,
        &content[end_idx + end_marker.len()..]
    )
}

fn generate_metrics_table(metrics: &[shared::metrics_defs::MetricDef]) -> String {
    let mut lines = vec![
        "| Metric | Type | Description |".to_string(),
        "|--------|------|-------------|".to_string(),
    ];
    for m in metrics {
        lines.push(format!(
            "| `{}` | {} | {} |",
            m.name,
            m.metric_type.as_str(),
            m.description
        ));
    }
    lines.join("\n")
}

pub fn init_statsd_recorder(prefix: &str, metrics_config: Option<MetricsConfig>) {
    if let Some(MetricsConfig {
        statsd_host,
        statsd_port,
    }) = metrics_config
    {
        let recorder = StatsdBuilder::from(statsd_host, statsd_port)
            .build(Some(prefix))
            .expect("Could not create StatsdRecorder");

        metrics::set_global_recorder(recorder).expect("Could not set global metrics recorder")
    }
}

fn run_async(
    fut: impl Future<Output = Result<(), impl std::error::Error>>,
) -> Result<(), CliError> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    if let Err(e) = rt.block_on(fut) {
        tracing::error!(error = %e, "Runtime error");
        process::exit(1);
    }
    Ok(())
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with(sentry::integrations::tracing::layer())
        .init();
}

fn init_sentry(logging_config: Option<config::LoggingConfig>) -> Option<sentry::ClientInitGuard> {
    // Initialize Sentry client if configured
    // The Sentry tracing layer (already initialized in main) will automatically
    // start sending events to Sentry once this client is initialized
    logging_config.map(|cfg| {
        sentry::init((
            cfg.sentry_dsn,
            sentry::ClientOptions {
                release: sentry::release_name!(),
                ..Default::default()
            },
        ))
    })
}

#[derive(Args, Debug, Clone)]
struct BaseArgs {
    #[arg(long)]
    config_file_path: PathBuf,
}

#[derive(Args, Debug)]
struct LocatorArgs {
    #[command(flatten)]
    base: BaseArgs,
}

#[derive(Args, Debug)]
struct ProxyArgs {
    #[command(flatten)]
    base: BaseArgs,
}

#[derive(Args, Debug)]
struct IngestRouterArgs {
    #[command(flatten)]
    base: BaseArgs,
}

#[cfg(test)]
mod tests {
    #[test]
    fn metrics_md_contains_all_defined_metrics() {
        let metrics_md =
            std::fs::read_to_string("../METRICS.md").expect("Failed to read METRICS.md");

        let mut missing = Vec::new();
        for m in locator::metrics_defs::ALL_METRICS {
            if !metrics_md.contains(m.name) {
                missing.push(m.name);
            }
        }

        assert!(
            missing.is_empty(),
            "METRICS.md is missing these metrics: {:?}\nAdd them to METRICS.md",
            missing
        );
    }
}
