//! Routing Infrastructure
//!
//! Provides the routing table abstraction for mapping locales to upstream targets.
//!
//! # Routing Model
//!
//! The routing system uses a two-level hierarchy:
//!
//! 1. **Locale → Cells**: Each locale (e.g., "us", "de") maps to a list of cell names
//! 2. **Cell → Upstream**: Each cell name maps to an `UpstreamTarget` with URLs
//!
//! ## Example
//!
//! ```text
//! Locale "us" → Cells ["us-1", "us-2"]
//!   ├─ "us-1" → UpstreamTarget {
//!   │    relay_url: "http://us1-relay.example.com",
//!   │    sentry_url: "http://us1-sentry.example.com"
//!   │  }
//!   └─ "us-2" → UpstreamTarget { ... }
//! ```
//!
//! The `RoutingTable` is built at startup from configuration and remains immutable
//! during request processing.

use std::collections::HashMap;
use url::Url;

use crate::config::CellConfig;

/// Represents a single upstream target with its URLs
#[derive(Clone, Debug)]
pub struct UpstreamTarget {
    /// Cell/upstream name
    pub name: String,
    /// Relay upstream URL (for reaching relay endpoints)
    pub relay_url: Url,
    /// Sentry upstream URL (for reaching sentry API endpoints)
    pub sentry_url: Url,
}

/// Collection of upstream targets grouped by cell name
#[derive(Clone, Debug)]
pub struct CellRegistry {
    cells: HashMap<String, UpstreamTarget>,
}

impl CellRegistry {
    /// Build a cell registry from cell configurations
    fn from_cells(cell_configs: HashMap<String, CellConfig>) -> Self {
        let cells = cell_configs
            .into_iter()
            .map(|(name, cell_config)| {
                let target = UpstreamTarget {
                    name: name.clone(),
                    relay_url: cell_config.relay_url,
                    sentry_url: cell_config.sentry_url,
                };
                (name, target)
            })
            .collect();
        Self { cells }
    }

    /// Get a specific target by cell name
    pub fn get(&self, cell_name: &str) -> Option<&UpstreamTarget> {
        self.cells.get(cell_name)
    }

    /// Get all cells as a HashMap
    pub fn cells(&self) -> &HashMap<String, UpstreamTarget> {
        &self.cells
    }
}

/// Routing table that maps locales to their upstream targets
#[derive(Clone, Debug)]
pub struct RoutingTable {
    /// Mapping from locale to cell registry
    locale_to_cells: HashMap<String, CellRegistry>,
}

impl RoutingTable {
    /// Build a routing table from locale cell configurations
    pub fn new(locales: HashMap<String, HashMap<String, CellConfig>>) -> Self {
        // Build locale -> cell registry mapping
        let locale_to_cells = locales
            .into_iter()
            .map(|(locale, cells)| {
                let registry = CellRegistry::from_cells(cells);
                (locale, registry)
            })
            .collect();

        Self { locale_to_cells }
    }

    /// Get the cell registry for a specific locale
    pub fn get_cell_registry(&self, locale: &str) -> Option<&CellRegistry> {
        self.locale_to_cells.get(locale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routing_table_creation() {
        let mut locales = HashMap::new();
        locales.insert(
            "us".to_string(),
            HashMap::from([
                (
                    "us1".to_string(),
                    CellConfig {
                        sentry_url: Url::parse("http://us1-sentry.example.com").unwrap(),
                        relay_url: Url::parse("http://us1-relay.example.com").unwrap(),
                    },
                ),
                (
                    "us2".to_string(),
                    CellConfig {
                        sentry_url: Url::parse("http://us2-sentry.example.com").unwrap(),
                        relay_url: Url::parse("http://us2-relay.example.com").unwrap(),
                    },
                ),
            ]),
        );
        locales.insert(
            "de".to_string(),
            HashMap::from([(
                "de".to_string(),
                CellConfig {
                    sentry_url: Url::parse("http://de-sentry.example.com").unwrap(),
                    relay_url: Url::parse("http://de-relay.example.com").unwrap(),
                },
            )]),
        );

        let routing_table = RoutingTable::new(locales);

        // Verify US locale has 2 cells
        let us_registry = routing_table.get_cell_registry("us").unwrap();
        assert_eq!(us_registry.cells().len(), 2);
        assert!(us_registry.get("us1").is_some());
        assert!(us_registry.get("us2").is_some());

        // Verify DE locale has 1 cell
        let de_registry = routing_table.get_cell_registry("de").unwrap();
        assert_eq!(de_registry.cells().len(), 1);
        assert!(de_registry.get("de").is_some());

        // Verify unknown locale returns None
        assert!(routing_table.get_cell_registry("unknown").is_none());
    }

    #[test]
    fn test_routing_table_with_all_cells() {
        // All cells are included in the locale config
        let mut locales = HashMap::new();
        locales.insert(
            "us".to_string(),
            HashMap::from([(
                "us1".to_string(),
                CellConfig {
                    sentry_url: Url::parse("http://us1-sentry.example.com").unwrap(),
                    relay_url: Url::parse("http://us1-relay.example.com").unwrap(),
                },
            )]),
        );

        let routing_table = RoutingTable::new(locales);

        // Should have the cell
        let us_registry = routing_table.get_cell_registry("us").unwrap();
        assert_eq!(us_registry.cells().len(), 1);
        assert!(us_registry.get("us1").is_some());
    }

    #[test]
    fn test_cell_registry_get() {
        let cells = HashMap::from([(
            "test-cell".to_string(),
            CellConfig {
                sentry_url: Url::parse("http://sentry.example.com").unwrap(),
                relay_url: Url::parse("http://relay.example.com").unwrap(),
            },
        )]);

        let registry = CellRegistry::from_cells(cells);

        // Test successful get
        let target = registry.get("test-cell").unwrap();
        assert_eq!(target.name, "test-cell");
        assert_eq!(target.sentry_url.as_str(), "http://sentry.example.com/");
        assert_eq!(target.relay_url.as_str(), "http://relay.example.com/");

        // Test missing cell
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_upstream_target_urls() {
        let target = UpstreamTarget {
            name: "test".to_string(),
            sentry_url: Url::parse("http://sentry.example.com:8080").unwrap(),
            relay_url: Url::parse("http://relay.example.com:8090").unwrap(),
        };

        assert_eq!(target.sentry_url.host_str(), Some("sentry.example.com"));
        assert_eq!(target.sentry_url.port(), Some(8080));
        assert_eq!(target.relay_url.host_str(), Some("relay.example.com"));
        assert_eq!(target.relay_url.port(), Some(8090));
    }
}
