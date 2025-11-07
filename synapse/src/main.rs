use clap::{Args, Parser};
use std::path::PathBuf;

mod config;
use config::Config;
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
            let _sentry_guard = init_sentry(config.common.logging);

            let locator_config = config.locator.ok_or(CliError::InvalidConfig(
                "Missing locator config".to_string(),
            ))?;
            run_async(locator::run(locator_config));
            Ok(())
        }
        CliCommand::Proxy(proxy_args) => {
            let config = Config::from_file(&proxy_args.base.config_file_path)?;
            let _sentry_guard = init_sentry(config.common.logging);

            let proxy_config = config.proxy.ok_or(CliError::InvalidConfig(
                "Missing proxy config".to_string(),
            ))?;


            run_async(proxy::run(proxy_config));
            Ok(())
        }
        CliCommand::IngestRouter(ingest_router_args) => {
            let config = Config::from_file(&ingest_router_args.base.config_file_path)?;
            let _sentry_guard = init_sentry(config.common.logging);

            let ingest_router_config = config
                .ingest_router
                .ok_or(CliError::InvalidConfig("Missing ingest-router config".to_string()))?;

            println!("Starting ingest-router with config {ingest_router_config:#?}");
            Ok(())
        }
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

fn init_sentry(logging_config: Option<config::LoggingConfig>) -> Option<sentry::ClientInitGuard> {
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
