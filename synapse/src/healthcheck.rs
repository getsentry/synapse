use crate::CliError;
use crate::config::Config;
use std::path::Path;

pub fn run(config_path: &Path) -> Result<(), CliError> {
    let config = Config::from_file(config_path)?;
    let port = config
        .proxy
        .as_ref()
        .map(|c| c.admin_listener.port)
        .or_else(|| config.ingest_router.as_ref().map(|c| c.admin_listener.port))
        .ok_or(CliError::InvalidConfig(
            "Missing proxy or ingest-router config",
        ))?;
    let response = reqwest::blocking::get(format!("http://localhost:{port}/ready"))
        .map_err(|e| CliError::HealthcheckFailed(e.to_string()))?;
    if !response.status().is_success() {
        return Err(CliError::HealthcheckFailed(format!(
            "status {}",
            response.status()
        )));
    }
    Ok(())
}
