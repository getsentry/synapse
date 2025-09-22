use crate::config::Route;

pub struct RouteActions {
    #[allow(dead_code)]
    routes: Vec<Route>,
}

impl RouteActions {
    pub fn new(routes: Vec<Route>) -> Self {
        Self { routes }
    }
}
