use crate::config::{Action, Route as RouteConfig};
use crate::errors::ProxyError;
use routing::RouteMatch;

impl TryFrom<RouteConfig> for routing::Route<Action> {
    type Error = ProxyError;
    fn try_from(config: RouteConfig) -> Result<Self, Self::Error> {
        // TODO: Add route validation
        Ok(routing::Route::new(
            config.r#match.host,
            config.r#match.path,
            config.action,
        ))
    }
}

pub struct RouteActions {
    inner: routing::RouteActions<Action>,
}

impl RouteActions {
    pub fn try_new(route_config: Vec<RouteConfig>) -> Result<Self, ProxyError> {
        let routes: Vec<routing::Route<Action>> = route_config
            .into_iter()
            .map(routing::Route::try_from)
            .collect::<Result<_, _>>()?;

        Ok(Self {
            inner: routing::RouteActions::new(routes),
        })
    }

    /// Matches the incoming request to a route, and returns the first matched route if any.
    /// If no matches are found, return none.
    pub fn resolve<'a, B>(
        &'a self,
        request: &'a http::Request<B>,
    ) -> Option<RouteMatch<'a, Action>> {
        println!("Resolving route for request URI: {:?}", request.uri());
        println!("Request path: {}", request.uri().path());
        println!("Request query: {:?}", request.uri().query());

        self.inner.resolve(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_host_only() {
        let config = RouteConfig {
            r#match: crate::config::Match {
                host: Some("sentry.io".to_string()),
                path: None,
            },
            action: crate::config::Action::Static {
                to: "upstream".to_string(),
            },
        };

        let route = routing::Route::try_from(config).unwrap();
        assert!(route.matches(Some("sentry.io"), "/").is_some());
        assert!(route.matches(Some("other.com"), "/").is_none());
        assert!(route.matches(None, "/").is_none());
    }

    #[test]
    fn test_static_path() {
        let config = RouteConfig {
            r#match: crate::config::Match {
                host: None,
                path: Some("/api/test/".to_string()),
            },
            action: crate::config::Action::Static {
                to: "upstream".to_string(),
            },
        };

        let route = routing::Route::try_from(config).unwrap();
        assert!(route.matches(None, "/api/test").is_some(), "exact path");
        assert!(
            route.matches(None, "/api/test/").is_some(),
            "with trailing slash normalization"
        );
        assert!(
            route.matches(None, "/api/test/2").is_none(),
            "extra segment doesn't match"
        );
        assert!(
            route.matches(None, "/api/").is_none(),
            "missing segment doesn't match"
        );
    }

    #[test]
    fn test_trailing_splat() {
        let config = RouteConfig {
            r#match: crate::config::Match {
                host: None,
                path: Some("/api/test/*".to_string()),
            },
            action: crate::config::Action::Static {
                to: "upstream".to_string(),
            },
        };

        let route = routing::Route::try_from(config).unwrap();
        assert!(route.matches(None, "/api/test").is_some(), "exact path");
        assert!(
            route.matches(None, "/api/test/extra").is_some(),
            "extra segment is allowed"
        );
        assert!(route.matches(None, "/api/").is_none(), "missing segment");
    }

    #[test]
    #[ignore]
    fn test_illegal_splat_patterns() {
        unimplemented!("add validation on route creation for illegal patterns");
    }

    #[test]
    fn test_dynamic_path() {
        let config = RouteConfig {
            r#match: crate::config::Match {
                host: None,
                path: Some("/api/users/{user_id}".to_string()),
            },
            action: crate::config::Action::Static {
                to: "upstream".to_string(),
            },
        };

        let route = routing::Route::try_from(config.clone()).unwrap();

        assert_eq!(
            route.matches(None, "/api/users/123"),
            Some(RouteMatch {
                params: HashMap::from([("user_id".to_string(), "123")]),
                action: &config.action,
            })
        );
    }
}
