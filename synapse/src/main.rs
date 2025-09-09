use clap::{Parser, Subcommand};


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
            println!("Starting locator");
            locator::run();
        }
        CliCommand::Proxy => {
            println!("Starting proxy");
        }
        CliCommand::IngestRouter => {
            println!("Starting ingest-router");
        }
    }

}

