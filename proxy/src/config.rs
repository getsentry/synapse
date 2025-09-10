use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Config {
    pub routes: Vec<Route>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Route {
    pub r#match: Match,
    pub route: RouteAction,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Match {
    pub host: String,
    pub path_prefix_pattern: Option<String>,
}

#[allow(dead_code)]
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
