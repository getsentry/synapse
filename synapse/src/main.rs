use clap::{Args, Parser};
use std::path::PathBuf;

mod config;
use config::Config;

#[derive(Parser)]
enum CliCommand {
    Locator(LocatorArgs),
    Proxy(ProxyArgs),
    IngestRouter,
}

fn main() {
    let cli = CliCommand::parse();

    match &cli {
        CliCommand::Locator(locator_args) => {
            let _config = Config::from_file(&locator_args.base.config_file_path)
                .expect("Failed to load config file");

            println!("Starting locator");
            locator::run();
        }
        CliCommand::Proxy(proxy_args) => {
            let config = Config::from_file(&proxy_args.base.config_file_path)
                .expect("Failed to load config file")
                .proxy
                .expect("Proxy config missing");
            proxy::run(config);
        }
        CliCommand::IngestRouter => {
            println!("Starting ingest-router");
        }
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
