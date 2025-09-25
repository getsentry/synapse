use crate::config::Route;
use hyper::body::Incoming;

pub struct RouteActions {
    #[allow(dead_code)]
    routes: Vec<Route>,
}

impl RouteActions {
    pub fn new(routes: Vec<Route>) -> Self {
        Self { routes }
    }
    /// Matches the incoming request to a route, and returns the first matched route if any.
    /// If no matches are found, return none.
    pub fn resolve(&self, _request: &http::Request<Incoming>) -> Option<&Route> {
        unimplemented!();
    }
}
