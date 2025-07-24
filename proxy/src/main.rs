use std::error::Error;

mod config;

fn main() -> Result<(), Box<dyn Error>> {
    let parsed_config = config::load_from_file("example_config.yaml")?;
    println!("\n--- Parsed Config ---\n{parsed_config:#?}");
    Ok(())
}
