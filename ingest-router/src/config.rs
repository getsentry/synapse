use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use url::Url;

#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Port cannot be 0")]
    InvalidPort,

    #[error("Route action references unknown locale: {0}")]
    UnknownLocale(String),

    #[error("Duplicate upstream name: {0}")]
    DuplicateUpstream(String),

    #[error("Empty upstream name")]
    EmptyUpstreamName,

    #[error("Empty locale in action")]
    EmptyLocale,
}

/// HTTP methods supported for route matching
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

/// Resolver types for handling requests
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResolverType {
    RelayMergeProjectConfigs,
}

/// Proxy configuration
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Config {
    /// Main listener for incoming requests
    pub listener: Listener,
    /// Admin listener for administrative endpoints
    pub admin_listener: Listener,
    /// Maps locale identifiers to lists of cell names
    ///
    /// Note: Uses String keys instead of an enum to allow flexible,
    /// deployment-specific locale configuration without code changes.
    /// Different deployments may use different locale identifiers.
    pub locale_to_cells: HashMap<String, Vec<String>>,
    /// List of upstream servers
    pub upstreams: Vec<UpstreamConfig>,
    /// Request routing rules
    pub routes: Vec<Route>,
}

impl Config {
    /// Validates the proxy configuration
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Validate listeners
        self.listener.validate()?;
        self.admin_listener.validate()?;

        // Validate upstreams
        let mut upstream_names = HashSet::new();
        for upstream in &self.upstreams {
            if upstream.name.is_empty() {
                return Err(ValidationError::EmptyUpstreamName);
            }

            if !upstream_names.insert(&upstream.name) {
                return Err(ValidationError::DuplicateUpstream(upstream.name.clone()));
            }
        }

        // Collect valid locales from locale_to_cells
        let valid_locales: HashSet<&String> = self.locale_to_cells.keys().collect();

        // Validate route actions
        for route in &self.routes {
            route.action.validate(&valid_locales)?;
        }

        Ok(())
    }
}

/// Network listener configuration
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Listener {
    /// Host address to bind to (e.g., "0.0.0.0" or "127.0.0.1")
    pub host: String,
    /// Port number to listen on
    pub port: u16,
}

impl Listener {
    /// Validates the listener configuration
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.port == 0 {
            return Err(ValidationError::InvalidPort);
        }
        Ok(())
    }
}

/// Upstream server configuration
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct UpstreamConfig {
    /// Unique identifier for this upstream
    pub name: String,
    /// URL of the upstream server
    ///
    /// Note: Uses the `url::Url` type for compile-time URL validation.
    /// Invalid URLs will be rejected during config deserialization.
    pub url: Url,
}

/// Routing rule configuration
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Route {
    /// Conditions for matching incoming requests
    pub r#match: Match,
    /// Action to take when the match conditions are met
    pub action: Action,
}

/// Request matching criteria
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Match {
    /// Optional hostname to match (e.g., "us.sentry.io")
    pub host: Option<String>,
    /// Optional path to match (e.g., "/api/0/relays/projectconfigs/")
    pub path: Option<String>,
    /// Optional HTTP method to match
    pub method: Option<HttpMethod>,
}

/// Action to perform when a route matches
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Action {
    /// Type of resolver to use for handling the request
    pub resolver: ResolverType,
    /// List of locale identifiers to route the request to
    pub locale: Vec<String>,
}

impl Action {
    /// Validates the action configuration
    pub fn validate(&self, valid_locales: &HashSet<&String>) -> Result<(), ValidationError> {
        if self.locale.is_empty() {
            return Err(ValidationError::EmptyLocale);
        }

        for locale in &self.locale {
            if !valid_locales.contains(locale) {
                return Err(ValidationError::UnknownLocale(locale.clone()));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_config() {
        let yaml = r#"
listener:
    host: "0.0.0.0"
    port: 3000
admin_listener:
    host: "127.0.0.1"
    port: 3001
locale_to_cells:
    us:
        - us1
        - us2
    de:
        - de1
upstreams:
    - name: us1
      url: "http://127.0.0.1:8080"
    - name: us2
      url: "http://10.0.0.2:8080"
routes:
    - match:
        host: us.sentry.io
        path: /api/0/relays/projectconfigs/
        method: POST
      action:
        resolver: relay_merge_project_configs
        locale:
          - us
    - match:
        path: /api/healthcheck
      action:
        resolver: relay_merge_project_configs
        locale:
          - us
          - de
"#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.validate().is_ok());

        // Verify key config values
        assert_eq!(config.listener.port, 3000);
        assert_eq!(config.upstreams.len(), 2);
        assert_eq!(config.routes.len(), 2);
        assert_eq!(config.routes[0].r#match.method, Some(HttpMethod::Post));
        assert_eq!(config.routes[1].r#match.host, None);
        assert_eq!(config.routes[1].action.locale.len(), 2);
    }

    #[test]
    fn test_validation_errors() {
        let base_config = Config {
            listener: Listener {
                host: "0.0.0.0".to_string(),
                port: 3000,
            },
            admin_listener: Listener {
                host: "127.0.0.1".to_string(),
                port: 3001,
            },
            locale_to_cells: HashMap::from([("us".to_string(), vec!["us1".to_string()])]),
            upstreams: vec![UpstreamConfig {
                name: "us1".to_string(),
                url: Url::parse("http://127.0.0.1:8080").unwrap(),
            }],
            routes: vec![Route {
                r#match: Match {
                    path: Some("/api/".to_string()),
                    host: None,
                    method: None,
                },
                action: Action {
                    resolver: ResolverType::RelayMergeProjectConfigs,
                    locale: vec!["us".to_string()],
                },
            }],
        };

        // Test invalid port
        let mut config = base_config.clone();
        config.listener.port = 0;
        assert!(matches!(
            config.validate().unwrap_err(),
            ValidationError::InvalidPort
        ));

        // Test duplicate upstream names
        let mut config = base_config.clone();
        config.upstreams.push(UpstreamConfig {
            name: "us1".to_string(),
            url: Url::parse("http://10.0.0.2:8080").unwrap(),
        });
        assert!(matches!(
            config.validate().unwrap_err(),
            ValidationError::DuplicateUpstream(_)
        ));

        // Test empty upstream name
        let mut config = base_config.clone();
        config.upstreams.push(UpstreamConfig {
            name: "".to_string(),
            url: Url::parse("http://10.0.0.2:8080").unwrap(),
        });
        assert!(matches!(
            config.validate().unwrap_err(),
            ValidationError::EmptyUpstreamName
        ));

        // Test unknown locale in action
        let mut config = base_config.clone();
        config.routes[0].action.locale = vec!["unknown".to_string()];
        assert!(matches!(
            config.validate().unwrap_err(),
            ValidationError::UnknownLocale(_)
        ));

        // Test empty locale in action
        let mut config = base_config;
        config.routes[0].action.locale = vec![];
        assert!(matches!(
            config.validate().unwrap_err(),
            ValidationError::EmptyLocale
        ));
    }

    #[test]
    fn test_deserialization_errors() {
        // Invalid URL
        assert!(
            serde_yaml::from_str::<Config>(
                r#"
listener: {host: "0.0.0.0", port: 3000}
admin_listener: {host: "127.0.0.1", port: 3001}
locale_to_cells: {us: [us1]}
upstreams: [{name: us1, url: "not-a-url"}]
routes: []
"#
            )
            .is_err()
        );

        // Invalid port type
        assert!(
            serde_yaml::from_str::<Config>(
                r#"
listener: {host: "0.0.0.0", port: "not_a_number"}
"#
            )
            .is_err()
        );

        // Missing required field
        assert!(
            serde_yaml::from_str::<Config>(
                r#"
listener: {host: "0.0.0.0"}
"#
            )
            .is_err()
        );

        // Invalid HTTP method
        assert!(serde_yaml::from_str::<HttpMethod>("INVALID_METHOD").is_err());

        // Invalid resolver type
        assert!(serde_yaml::from_str::<ResolverType>("invalid_resolver").is_err());
    }

    #[test]
    fn test_enum_deserialization() {
        // HTTP methods
        assert_eq!(
            serde_yaml::from_str::<HttpMethod>("GET").unwrap(),
            HttpMethod::Get
        );
        assert_eq!(
            serde_yaml::from_str::<HttpMethod>("POST").unwrap(),
            HttpMethod::Post
        );
        assert_eq!(
            serde_yaml::from_str::<HttpMethod>("PUT").unwrap(),
            HttpMethod::Put
        );
        assert_eq!(
            serde_yaml::from_str::<HttpMethod>("DELETE").unwrap(),
            HttpMethod::Delete
        );

        // Resolver types
        assert_eq!(
            serde_yaml::from_str::<ResolverType>("relay_merge_project_configs").unwrap(),
            ResolverType::RelayMergeProjectConfigs
        );
    }
}
