use clap::{Args, Parser};
use std::path::PathBuf;

mod config;
use config::Config;

#[derive(Parser)]
enum CliCommand {
    Locator(LocatorArgs),
    Proxy,
    IngestRouter,
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
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn cli() -> Result<(), CliError> {
    let cli = CliCommand::parse();

    match &cli {
        CliCommand::Locator(locator_args) => {
            let config = Config::from_file(&locator_args.base.config_file_path)?;
            let locator_config = config.locator.ok_or(CliError::InvalidConfig(
                "Missing locator config".to_string(),
            ))?;
            run_async(locator::run(locator_config));
            Ok(())
        }
        CliCommand::Proxy => {
            println!("Starting proxy");
            run_async(proxy::run());
            Ok(())
        }
        CliCommand::IngestRouter => {
            println!("Starting ingest-router");
            Ok(())
        }
    }
}



pub fn run_async(fut: impl Future<Output = ()>) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(fut);
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
