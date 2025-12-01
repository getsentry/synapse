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

    #[error("Locale '{0}' has no valid cells (none of its cells match any upstream)")]
    LocaleHasNoValidCells(String),
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

impl PartialEq<hyper::Method> for HttpMethod {
    fn eq(&self, other: &hyper::Method) -> bool {
        match self {
            HttpMethod::Get => other == hyper::Method::GET,
            HttpMethod::Post => other == hyper::Method::POST,
            HttpMethod::Put => other == hyper::Method::PUT,
            HttpMethod::Delete => other == hyper::Method::DELETE,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "handler", content = "args", rename_all = "snake_case")]
pub enum HandlerAction {
    /// Merges project configs from multiple relay instances
    RelayProjectConfigs(RelayProjectConfigsArgs),
}

/// Arguments for the relay_project_configs handler
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RelayProjectConfigsArgs {
    /// Locale identifier to route the request to
    pub locale: String,
}

/// Cell/upstream configuration
/// Note: The cell name is the HashMap key in Config.locales
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CellConfig {
    /// Name/identifier of the cell
    pub name: String,
    /// URL of the Sentry upstream server
    pub sentry_url: Url,
    /// URL of the Relay upstream server
    pub relay_url: Url,
}

/// Proxy configuration
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Config {
    /// Main listener for incoming requests
    #[serde(default)]
    pub listener: Listener,
    /// Admin listener for administrative endpoints
    #[serde(default)]
    pub admin_listener: AdminListener,
    /// Cells are stored as a Vec to maintain priority order - the first cell
    /// in the list has highest priority for global config responses.
    pub locales: HashMap<String, Vec<CellConfig>>,
    /// Request routing rules
    pub routes: Vec<Route>,
}

impl Config {
    /// Validates the proxy configuration
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Validate listeners
        self.listener.validate()?;
        self.admin_listener.validate()?;

        // Validate locales and cells
        for (locale, cells) in &self.locales {
            // Check that locale has at least one cell
            if cells.is_empty() {
                return Err(ValidationError::LocaleHasNoValidCells(locale.clone()));
            }

            // Check for empty cell names and collect for duplicate checking
            let mut seen_names = HashSet::new();
            for cell in cells {
                if cell.name.is_empty() {
                    return Err(ValidationError::EmptyUpstreamName);
                }
                if !seen_names.insert(&cell.name) {
                    return Err(ValidationError::DuplicateUpstream(cell.name.clone()));
                }
            }
        }

        // Collect valid locales
        let valid_locales: HashSet<&String> = self.locales.keys().collect();

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

impl Default for Listener {
    fn default() -> Self {
        Listener {
            host: "0.0.0.0".into(),
            port: 3000,
        }
    }
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

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct AdminListener {
    /// Host address to bind to (e.g., "0.0.0.0" or "127.0.0.1")
    pub host: String,
    /// Port number to listen on
    pub port: u16,
}

impl Default for AdminListener {
    fn default() -> Self {
        AdminListener {
            host: "0.0.0.0".into(),
            port: 3001,
        }
    }
}

impl AdminListener {
    /// Validates the admin listener configuration
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.port == 0 {
            return Err(ValidationError::InvalidPort);
        }
        Ok(())
    }
}

/// Routing rule configuration
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Route {
    /// Conditions for matching incoming requests
    pub r#match: Match,
    /// Action to take when the match conditions are met
    pub action: HandlerAction,
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

impl HandlerAction {
    /// Validates the handler action configuration
    pub fn validate(&self, valid_locales: &HashSet<&String>) -> Result<(), ValidationError> {
        match self {
            HandlerAction::RelayProjectConfigs(args) => {
                if !valid_locales.contains(&args.locale) {
                    return Err(ValidationError::UnknownLocale(args.locale.clone()));
                }
                Ok(())
            }
        }
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
locales:
    us:
        - name: us1
          sentry_url: "http://127.0.0.1:8080"
          relay_url: "http://127.0.0.1:8090"
        - name: us2
          sentry_url: "http://10.0.0.2:8080"
          relay_url: "http://10.0.0.2:8090"
    de:
        - name: de1
          sentry_url: "http://10.0.0.3:8080"
          relay_url: "http://10.0.0.3:8090"
routes:
    - match:
        host: us.sentry.io
        path: /api/0/relays/projectconfigs/
        method: POST
      action:
        handler: relay_project_configs
        args:
          locale: us
    - match:
        path: /api/healthcheck
      action:
        handler: relay_project_configs
        args:
          locale: us
"#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.validate().is_ok());

        // Verify key config values
        assert_eq!(config.listener.port, 3000);
        assert_eq!(config.locales.len(), 2);
        assert_eq!(config.locales.get("us").unwrap().len(), 2);
        assert_eq!(config.locales.get("de").unwrap().len(), 1);
        assert_eq!(config.locales.get("us").unwrap()[0].name, "us1");
        assert_eq!(config.locales.get("us").unwrap()[1].name, "us2");
        assert_eq!(config.routes.len(), 2);
        assert_eq!(config.routes[0].r#match.method, Some(HttpMethod::Post));
        assert_eq!(config.routes[1].r#match.host, None);
        // Verify the handler action structure
        match &config.routes[1].action {
            HandlerAction::RelayProjectConfigs(args) => {
                assert_eq!(args.locale, "us");
            }
        }
    }

    #[test]
    fn test_validation_errors() {
        let base_config = Config {
            listener: Listener {
                host: "0.0.0.0".to_string(),
                port: 3000,
            },
            admin_listener: AdminListener {
                host: "127.0.0.1".to_string(),
                port: 3001,
            },
            locales: HashMap::from([(
                "us".to_string(),
                vec![CellConfig {
                    name: "us1".to_string(),
                    sentry_url: Url::parse("http://127.0.0.1:8080").unwrap(),
                    relay_url: Url::parse("http://127.0.0.1:8090").unwrap(),
                }],
            )]),
            routes: vec![Route {
                r#match: Match {
                    path: Some("/api/".to_string()),
                    host: None,
                    method: None,
                },
                action: HandlerAction::RelayProjectConfigs(RelayProjectConfigsArgs {
                    locale: "us".to_string(),
                }),
            }],
        };

        // Test invalid port
        let mut config = base_config.clone();
        config.listener.port = 0;
        assert!(matches!(
            config.validate().unwrap_err(),
            ValidationError::InvalidPort
        ));

        // Test empty cell name
        let mut config = base_config.clone();
        config.locales.get_mut("us").unwrap().push(CellConfig {
            name: "".to_string(),
            sentry_url: Url::parse("http://10.0.0.2:8080").unwrap(),
            relay_url: Url::parse("http://10.0.0.2:8090").unwrap(),
        });
        assert!(matches!(
            config.validate().unwrap_err(),
            ValidationError::EmptyUpstreamName
        ));

        // Test duplicate cell name
        let mut config = base_config.clone();
        config.locales.get_mut("us").unwrap().push(CellConfig {
            name: "us1".to_string(),
            sentry_url: Url::parse("http://10.0.0.2:8080").unwrap(),
            relay_url: Url::parse("http://10.0.0.2:8090").unwrap(),
        });
        assert!(matches!(
            config.validate().unwrap_err(),
            ValidationError::DuplicateUpstream(_)
        ));

        // Test unknown locale in action
        let mut config = base_config.clone();
        config.routes[0].action = HandlerAction::RelayProjectConfigs(RelayProjectConfigsArgs {
            locale: "unknown".to_string(),
        });
        assert!(matches!(
            config.validate().unwrap_err(),
            ValidationError::UnknownLocale(_)
        ));

        // Test locale with no cells
        let mut config = base_config.clone();
        config
            .locales
            .insert("invalid_locale".to_string(), Vec::new());
        assert!(matches!(
            config.validate().unwrap_err(),
            ValidationError::LocaleHasNoValidCells(_)
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
upstreams: [{name: us1, sentry_url: "not-a-url", relay_url: "http://127.0.0.1:8090"}]
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

        // Invalid handler type
        assert!(serde_yaml::from_str::<HandlerAction>(r#"handler: invalid_handler"#).is_err());

        // Missing required args field
        assert!(
            serde_yaml::from_str::<HandlerAction>(r#"handler: relay_project_configs"#).is_err()
        );
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

        // Handler action deserialization
        let action: HandlerAction = serde_yaml::from_str(
            r#"
handler: relay_project_configs
args:
  locale: us
"#,
        )
        .unwrap();
        match action {
            HandlerAction::RelayProjectConfigs(args) => {
                assert_eq!(args.locale, "us");
            }
        }
    }
}
