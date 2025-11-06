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

impl Path {
    /// Parses a path pattern string into a Path struct
    /// Supports:
    /// - Static segments: "/api/users"
    /// - Dynamic parameters: "/api/users/{id}"
    /// - Trailing splat: "/api/users/*"
    pub fn parse(path_str: &str) -> Self {
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
                    if let Some(stripped) = s.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                        PathSegment::Param(stripped.to_string())
                    } else {
                        PathSegment::Static(s.to_string())
                    }
                })
                .collect()
        };

        Path {
            segments,
            has_trailing_splat,
        }
    }

    /// Matches a request path against this path pattern
    /// Returns Some(params) if match succeeds, None otherwise
    fn matches<'a>(&self, request_path: &'a str) -> Option<HashMap<String, &'a str>> {
        let normalized_path = request_path.trim().trim_matches('/');

        let request_segments: Vec<&'a str> = if normalized_path.is_empty() {
            vec![]
        } else {
            normalized_path.split('/').collect()
        };

        let mut params = HashMap::new();
        let mut i_req = 0;

        for seg in self.segments.iter() {
            match seg {
                PathSegment::Static(s) => {
                    let req_segment = *request_segments.get(i_req)?;
                    if req_segment != s {
                        return None;
                    }
                    i_req += 1;
                }
                PathSegment::Param(name) => {
                    let req_segment = *request_segments.get(i_req)?;
                    params.insert(name.clone(), req_segment);
                    i_req += 1;
                }
            }
        }

        if self.has_trailing_splat || i_req == request_segments.len() {
            Some(params)
        } else {
            None
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct RouteMatch<'a, A> {
    pub params: HashMap<String, &'a str>,
    pub action: &'a A,
}

#[derive(Debug)]
pub struct Route<A> {
    host: Option<String>,
    path: Option<Path>,
    action: A,
}

impl<A> Route<A> {
    /// Creates a new Route with the given host, path, and action
    pub fn new(host: Option<String>, path: Option<String>, action: A) -> Self {
        let parsed_path = path.map(|p| Path::parse(&p));
        Self {
            host,
            path: parsed_path,
            action,
        }
    }

    /// Returns Some(RouteMatch) if the request matches this route, None otherwise.
    /// Trailing slash normalization is applied to incoming requests.
    pub fn matches<'a>(
        &'a self,
        request_host: Option<&str>,
        request_path: &'a str,
    ) -> Option<RouteMatch<'a, A>> {
        if self.host.is_some() && self.host.as_deref() != request_host {
            return None;
        }

        match &self.path {
            Some(path) => {
                let params = path.matches(request_path)?;
                Some(RouteMatch {
                    params,
                    action: &self.action,
                })
            }
            None => {
                // If no path is defined in the route, it matches anything
                Some(RouteMatch {
                    params: HashMap::new(),
                    action: &self.action,
                })
            }
        }
    }
}

pub struct RouteActions<A> {
    routes: Vec<Route<A>>,
}

impl<A> RouteActions<A> {
    pub fn new(routes: Vec<Route<A>>) -> Self {
        Self { routes }
    }

    /// Matches the incoming request to a route, and returns the first matched route if any.
    /// If no matches are found, return none.
    pub fn resolve<'a, B>(&'a self, request: &'a http::Request<B>) -> Option<RouteMatch<'a, A>> {
        // Host may come from authority part of URI (if absolute-form request)
        // or from the Host header (most common in HTTP/1.1).
        let host = request
            .uri()
            .host()
            .or_else(|| request.headers().get("host").and_then(|h| h.to_str().ok()));

        let path = request.uri().path();

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
        let route = Route::new(Some("sentry.io".to_string()), None, "upstream");
        assert!(route.matches(Some("sentry.io"), "/").is_some());
        assert!(route.matches(Some("other.com"), "/").is_none());
        assert!(route.matches(None, "/").is_none());
    }

    #[test]
    fn test_static_path() {
        let route = Route::new(None, Some("/api/test/".to_string()), "upstream");
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
        let route = Route::new(None, Some("/api/test/*".to_string()), "upstream");
        assert!(route.matches(None, "/api/test").is_some(), "exact path");
        assert!(
            route.matches(None, "/api/test/extra").is_some(),
            "extra segment is allowed"
        );
        assert!(route.matches(None, "/api/").is_none(), "missing segment");
    }

    #[test]
    fn test_dynamic_path() {
        let route = Route::new(None, Some("/api/users/{user_id}".to_string()), "upstream");

        let result = route.matches(None, "/api/users/123");
        assert!(result.is_some());
        let route_match = result.unwrap();
        assert_eq!(route_match.params.get("user_id").copied(), Some("123"));
        assert_eq!(route_match.action, &"upstream");
    }

    #[test]
    fn test_path_parsing() {
        // Test empty path
        let path = Path::parse("");
        assert_eq!(path.segments.len(), 0);
        assert!(!path.has_trailing_splat);

        // Test static path
        let path = Path::parse("/api/users");
        assert_eq!(path.segments.len(), 2);
        assert!(!path.has_trailing_splat);

        // Test dynamic path
        let path = Path::parse("/api/users/{id}");
        assert_eq!(path.segments.len(), 3);
        assert!(!path.has_trailing_splat);

        // Test trailing splat
        let path = Path::parse("/api/users/*");
        assert_eq!(path.segments.len(), 2);
        assert!(path.has_trailing_splat);
    }
}
