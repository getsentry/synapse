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

fn main() {
    let cli = CliCommand::parse();

    match &cli {
        CliCommand::Locator(locator_args) => {
            let locator_config = Config::from_file(&locator_args.base.config_file_path)
                .expect("Failed to load config file")
                .locator
                .unwrap();

            println!("Starting locator");
            run_async(locator::run(locator_config));
        }
        CliCommand::Proxy => {
            println!("Starting proxy");
            run_async(proxy::run());
        }
        CliCommand::IngestRouter => {
            println!("Starting ingest-router");
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
