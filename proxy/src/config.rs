use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub routes: Vec<Route>,
}
#[derive(Debug, Deserialize, Clone)]
pub struct Route {
    #[serde(rename = "match")]
    pub match_criteria: Match,
    #[serde(rename = "route")]
    pub route_action: RouteAction,
}

pub struct Match {
    pub host: String,
    pub path_prefix_pattern: String,
}

pub struct RouteAction {
    pub dynamic_to: Option<String>,
    pub to: Option<String>,
    pub default: Option<String>,
}