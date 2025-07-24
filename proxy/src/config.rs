use serde::Deserialize;
use std::error::Error;
use std::fs::File;

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
    pub path_prefix_pattern: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum RouteAction {
    Dynamic {
        dynamic_to: String,
        default: Option<String>,
    },
    Static {
        to: String,
    },
}

pub fn load_from_file(path: &str) -> Result<Config, Box<dyn Error>> {
    let file = File::open(path)?;
    let parsed_config: Config = serde_yaml::from_reader(file)?;
    Ok(parsed_config)
}
