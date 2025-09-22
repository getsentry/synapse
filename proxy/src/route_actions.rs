use crate::config::Route;

pub struct RouteActions {
    routes: Vec<Route>,
}

impl RouteActions {
    pub fn new(routes: Vec<Route>) -> Self {
        Self { routes }
    }
}
