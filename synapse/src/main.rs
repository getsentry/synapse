use clap::{Args, Parser};
use std::path::PathBuf;

mod config;
use config::{Config, MetricsConfig};
use metrics_exporter_statsd::StatsdBuilder;
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
    InvalidConfig(&'static str),
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
            init_statsd_recorder("synapse.locator", config.common.metrics);

            let locator_config = config
                .locator
                .ok_or(CliError::InvalidConfig("Missing locator config"))?;

            run_async(locator::run(locator_config));
            Ok(())
        }
        CliCommand::Proxy(proxy_args) => {
            let config = Config::from_file(&proxy_args.base.config_file_path)?;
            init_statsd_recorder("synapse.proxy", config.common.metrics);

            let proxy_config = config
                .proxy
                .ok_or(CliError::InvalidConfig("Missing proxy config"))?;

            run_async(proxy::run(proxy_config));
            Ok(())
        }
        CliCommand::IngestRouter(ingest_router_args) => {
            let config = Config::from_file(&ingest_router_args.base.config_file_path)?;
            init_statsd_recorder("synapse.ingest_router", config.common.metrics);

            let ingest_router_config = config
                .ingest_router
                .ok_or(CliError::InvalidConfig("Missing ingest-router config"))?;

            println!("Starting ingest-router with config {ingest_router_config:#?}");

            Ok(())
        }
    }
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
