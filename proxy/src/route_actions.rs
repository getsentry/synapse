use crate::config::{Action, Route as RouteConfig};
use crate::errors::ProxyError;
use std::collections::HashMap;

#[derive(Debug)]
enum PathSegment {
    Static(String),
    Param(String),
}

#[derive(Debug)]
struct Path {
    segments: Vec<PathSegment>,
    has_trailing_splat: bool,
}

#[derive(Debug, PartialEq)]
pub struct RouteMatch {
    pub params: HashMap<String, String>,
    pub action: Action,
}

#[derive(Debug)]
struct Route {
    host: Option<String>,
    path: Option<Path>,
    action: Action,
}

impl Route {
    // Returns Some(RouteMatch) if the request matches this route, None otherwise.
    // Trailing slash normalization is applied to incoming requests.
    fn matches(&self, request_host: Option<&str>, request_path: &str) -> Option<RouteMatch> {
        if self.host.is_some() && self.host.as_deref() != request_host {
            return None;
        }

        let normalized_path = request_path.trim().trim_matches('/');

        let request_segments = if normalized_path.is_empty() {
            vec![]
        } else {
            normalized_path.split('/').collect()
        };

        let mut params = HashMap::new();
        let mut i_req = 0;

        match &self.path {
            Some(path) => {
                for seg in path.segments.iter() {
                    match seg {
                        PathSegment::Static(s) => {
                            let req_segment = request_segments.get(i_req)?;
                            if req_segment != s {
                                return None;
                            }
                            i_req += 1;
                        }
                        PathSegment::Param(name) => {
                            let req_segment = request_segments.get(i_req)?;
                            params.insert(name.to_string(), req_segment.to_string());
                            i_req += 1;
                        }
                    }
                }

                if path.has_trailing_splat || i_req == request_segments.len() {
                    Some(RouteMatch {
                        params,
                        action: self.action.clone(),
                    })
                } else {
                    None
                }
            }
            None => {
                // If no path is defined in the route, it matches anything
                Some(RouteMatch {
                    params,
                    action: self.action.clone(),
                })
            }
        }
    }
}

impl TryFrom<RouteConfig> for Route {
    type Error = ProxyError;
    fn try_from(config: RouteConfig) -> Result<Self, Self::Error> {
        let is_static_action = matches!(config.action, Action::Static { .. });

        let path = match config.r#match.path {
            Some(path_str) => {
                // Trim slashes
                let mut normalized_path = path_str.trim().trim_matches('/');

                // Handle trailing splat
                let mut has_trailing_splat = false;
                if let Some(stripped) = normalized_path.strip_suffix("/*") {
                    has_trailing_splat = true;
                    normalized_path = stripped;
                }

                let segments: Vec<PathSegment> = if normalized_path.is_empty() {
                    vec![]
                } else {
                    normalized_path
                        .split('/')
                        .map(|s| {
                            if let Some(stripped) =
                                s.strip_prefix('{').and_then(|s| s.strip_suffix('}'))
                            {
                                if is_static_action {
                                    return Err(ProxyError::InvalidRoute(
                                        "Dynamic path parameters are not allowed with static actions"
                                            .to_string(),
                                    ));
                                }
                                let is_valid = stripped.chars().all(|ch| ch.is_ascii_lowercase());
                                if !is_valid {
                                    return Err(ProxyError::InvalidRoute(format!(
                                        "Invalid parameter name: {}",
                                        stripped
                                    )));
                                }

                                Ok(PathSegment::Param(stripped.to_string()))
                            } else {
                                let is_valid = s.chars().all(|ch| {
                                    ch.is_ascii_alphanumeric()
                                        || ch == '-'
                                        || ch == '_'
                                        || ch == '.'
                                });

                                if !is_valid {
                                    return Err(ProxyError::InvalidRoute(format!(
                                        "Invalid static segment: {}",
                                        s
                                    )));
                                }

                                Ok(PathSegment::Static(s.to_string()))
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?
                };
                Some(Path {
                    segments,
                    has_trailing_splat,
                })
            }
            None => None,
        };

        Ok(Self {
            host: config.r#match.host,
            path,
            action: config.action,
        })
    }
}

pub struct RouteActions {
    routes: Vec<Route>,
}

impl RouteActions {
    pub fn try_new(route_config: Vec<RouteConfig>) -> Result<Self, ProxyError> {
        let routes: Vec<Route> = route_config
            .into_iter()
            .map(Route::try_from)
            .collect::<Result<_, _>>()?;

        Ok(Self { routes })
    }
    /// Matches the incoming request to a route, and returns the first matched route if any.
    /// If no matches are found, return none.
    pub fn resolve<B>(&self, request: &http::Request<B>) -> Option<RouteMatch> {
        tracing::debug!("Resolving route for request URI: {:?}", request.uri());

        // Host may come from authority part of URI (if absolute-form request)
        // or from the Host header (most common in HTTP/1.1).
        let host = request
            .uri()
            .host()
            .or_else(|| request.headers().get("host").and_then(|h| h.to_str().ok()));

        let path = request.uri().path();
        let query = request.uri().query();

        tracing::debug!("Request path: {path}");
        tracing::debug!("Request query: {query:?}");

        // Return the first matching route, if any
        self.routes
            .iter()
            .find_map(|route| route.matches(host, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let route = Route::try_from(config).unwrap();
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

        let route = Route::try_from(config).unwrap();
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

        let route = Route::try_from(config).unwrap();
        assert!(route.matches(None, "/api/test").is_some(), "exact path");
        assert!(
            route.matches(None, "/api/test/extra").is_some(),
            "extra segment is allowed"
        );
        assert!(route.matches(None, "/api/").is_none(), "missing segment");
    }

    #[test]
    fn test_illegal_splat_patterns() {
        // Splat appears in the midele
        let config = RouteConfig {
            r#match: crate::config::Match {
                host: None,
                path: Some("/api/*/test".to_string()),
            },
            action: crate::config::Action::Static {
                to: "upstream".to_string(),
            },
        };
        assert!(
            Route::try_from(config).is_err(),
            "splat in the middle should be rejected"
        );

        // Multiple splats
        let config = RouteConfig {
            r#match: crate::config::Match {
                host: None,
                path: Some("/api/*/*".to_string()),
            },
            action: crate::config::Action::Static {
                to: "upstream".to_string(),
            },
        };
        assert!(
            Route::try_from(config).is_err(),
            "multiple splats should be rejected"
        );

        // Splat mixed with text in a segment
        let config = RouteConfig {
            r#match: crate::config::Match {
                host: None,
                path: Some("/api/test*/more".to_string()),
            },
            action: crate::config::Action::Static {
                to: "upstream".to_string(),
            },
        };
        assert!(
            Route::try_from(config).is_err(),
            "splat mixed with text should be rejected"
        );

        // Double splat
        let config = RouteConfig {
            r#match: crate::config::Match {
                host: None,
                path: Some("/api/**".to_string()),
            },
            action: crate::config::Action::Static {
                to: "upstream".to_string(),
            },
        };
        assert!(
            Route::try_from(config).is_err(),
            "double splat should be rejected"
        );

        // Splat with parameter syntax
        let config = RouteConfig {
            r#match: crate::config::Match {
                host: None,
                path: Some("/api/{*splat}".to_string()),
            },
            action: crate::config::Action::Static {
                to: "upstream".to_string(),
            },
        };
        assert!(
            Route::try_from(config).is_err(),
            "splat with parameter syntax should be rejected"
        );
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

        let route = Route::try_from(config.clone()).unwrap();

        assert_eq!(
            route.matches(None, "/api/users/123"),
            Some(RouteMatch {
                params: HashMap::from([("user_id".to_string(), "123".to_string())]),
                action: config.action.clone(),
            })
        );
    }
}
