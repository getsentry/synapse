use clap::{Args, Parser};
use std::path::PathBuf;

mod config;
use config::{Config, MetricsConfig};
use shared::metrics::Metrics;
use std::process;

#[derive(Parser)]
enum CliCommand {
    Locator(LocatorArgs),
    Proxy(ProxyArgs),
    IngestRouter(IngestRouterArgs),
}

#[derive(thiserror::Error, Debug)]
enum CliError {
    #[error("Failed to load config file: {0}")]
    ConfigLoadError(#[from] config::ConfigError),
    #[error("Invalid config: {0}")]
    InvalidConfig(String),
}

fn main() {
    if let Err(e) = cli() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn cli() -> Result<(), CliError> {
    let cmd = CliCommand::parse();

    match &cmd {
        CliCommand::Locator(locator_args) => {
            let config = Config::from_file(&locator_args.base.config_file_path)?;
            let locator_config = config.locator.ok_or(CliError::InvalidConfig(
                "Missing locator config".to_string(),
            ))?;

            let metrics = metrics_from_config(config.common.metrics, "synapse.locator");

            run_async(locator::run(locator_config, metrics));
            Ok(())
        }
        CliCommand::Proxy(proxy_args) => {
            let config = Config::from_file(&proxy_args.base.config_file_path)?;
            let proxy_config = config
                .proxy
                .ok_or(CliError::InvalidConfig("Missing proxy config".to_string()))?;

            let metrics = metrics_from_config(config.common.metrics, "synapse.proxy");
            run_async(proxy::run(proxy_config, metrics));
            Ok(())
        }
        CliCommand::IngestRouter(ingest_router_args) => {
            let config = Config::from_file(&ingest_router_args.base.config_file_path)?;

            let ingest_router_config = config.ingest_router.ok_or(CliError::InvalidConfig(
                "Missing ingest-router config".to_string(),
            ))?;
            let _metrics = metrics_from_config(config.common.metrics, "synapse.ingest-router");

            println!("Starting ingest-router with config {ingest_router_config:#?}");

            Ok(())
        }
    }
}

fn metrics_from_config(config: Option<MetricsConfig>, prefix: &str) -> Metrics {
    match config {
        Some(c) => Metrics::new(c.statsd_host, c.statsd_port, prefix)
            .expect("Failed to create metrics client"),
        None => Metrics::new_noop(),
    }
}

pub fn run_async(fut: impl Future<Output = Result<(), impl std::error::Error>>) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    if let Err(e) = rt.block_on(fut) {
        eprintln!("Error: {e}");
        process::exit(1);
    }
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
