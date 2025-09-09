use clap::{Parser, Subcommand};
use locator::run;


#[derive(Parser)]
enum CliCommand {
    Locator,
    Proxy,
    IngestRouter,
}



fn main() {
    let cli = CliCommand::parse();

    match &cli {
        CliCommand::Locator => {
            println!("run locator");
        }
        CliCommand::Proxy => {
            println!("run proxy");
        }
        CliCommand::IngestRouter => {
            println!("run ingest router");
        }
    }

}

