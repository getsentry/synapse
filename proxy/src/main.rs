use std::error::Error;
use std::net::SocketAddr;

mod config;
mod proxy;
mod rules_engine;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let parsed_config = config::load_from_file("example_config.yaml")?;
    println!("Loaded configuration with {} routes", parsed_config.routes.len());
    
    let proxy_server = proxy::ProxyServer::new(parsed_config);
    let addr: SocketAddr = "127.0.0.1:8080".parse()?;
    
    println!("Starting proxy server on {}", addr);
    proxy_server.run(addr).await?;
    
    Ok(())
}