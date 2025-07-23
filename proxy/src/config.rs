use serde::Deserialize;
use std::fs::File;
use std::error::Error;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub routes: Vec<Route>,
}

#[derive(Debug, Deserialize)]
pub struct Route {
    #[serde(rename = "match")]
    pub match_criteria: Match,
    #[serde(rename = "route")]
    pub route_action: RouteAction,
}

#[derive(Debug, Deserialize)]
pub struct Match {
    pub host: String,
    pub path_prefix_pattern: String,
}

#[derive(Debug, Deserialize)]
pub struct RouteAction {
    pub dynamic_to: Option<String>,
    pub to: Option<String>,
    pub default: Option<String>,
}


pub fn load_from_file(path: &str) -> Result<Config, Box<dyn Error>> {
    let file = File::open(path)?;
    let parsed_config: Config = serde_yaml::from_reader(file)?;
    Ok(parsed_config)
}
