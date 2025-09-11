
use crate::config::{Route, RouteAction};

pub struct IncomingRequest<'a> {
    pub host: &'a str,
    pub path: &'a str,
}

pub struct RulesEngine {
    pub routes: Vec<Route>,
}

impl RulesEngine {
    pub fn new(routes: Vec<Route>) -> Self {
        Self { routes }
    }
    
    // finds the route destination that matches the incoming request
    pub fn find_destination(&self, request: &IncomingRequest) -> Option<String> {
        for route in &self.routes {
            if route.r#match.host != request.host {
                continue;
            }

            let path_matches = match &route.r#match.path_prefix_pattern {
                Some(pattern) => {
                    let static_prefix = pattern.split('{').next().unwrap_or("");
                    request.path.starts_with(static_prefix)
                },
                // If no pattern is defined, any path is considered a match.
                None => true
            };
            
            if !path_matches {
                continue;
            }

            return match &route.route {
                RouteAction::Static { to } => Some(to.clone()),
                RouteAction::Dynamic { default, .. } => default.clone(),
            };
        }
        None
    }
}
