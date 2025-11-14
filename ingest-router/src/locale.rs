//! Locale-based Routing Infrastructure
//!
//! Maps locales to cells and cells to upstreams for request routing.
//!
//! # Model
//!
//! The routing system uses a two-level hierarchy:
//!
//! 1. **Locale → Cells**: Each locale (e.g., "us", "de") maps to cell names
//! 2. **Cell → Upstream**: Each cell name maps to an `Upstream` with URLs
//!
//! ## Example
//!
//! ```text
//! Locale "us" → Cells ["us-1", "us-2"]
//!   ├─ "us-1" → Upstream {
//!   │    relay_url: "http://us1-relay.example.com",
//!   │    sentry_url: "http://us1-sentry.example.com"
//!   │  }
//!   └─ "us-2" → Upstream { ... }
//! ```
//!
//! The `Locale` is built at startup from configuration and remains immutable
//! during request processing.

use std::collections::HashMap;
use url::Url;

use crate::config::CellConfig;

/// Represents a single upstream with its URLs
#[derive(Clone, Debug)]
pub struct Upstream {
    /// Relay URL for reaching relay endpoints
    pub relay_url: Url,
    /// Sentry URL for reaching sentry API endpoints
    pub sentry_url: Url,
}

impl From<CellConfig> for Upstream {
    fn from(config: CellConfig) -> Self {
        Self {
            relay_url: config.relay_url,
            sentry_url: config.sentry_url,
        }
    }
}

/// Collection of upstreams grouped by cell name
#[derive(Clone, Debug)]
pub struct Cells {
    pub cell_to_upstreams: HashMap<String, Upstream>,
}

impl Cells {
    /// Build cells from cell configurations
    fn from_config(cell_configs: HashMap<String, CellConfig>) -> Self {
        Self {
            cell_to_upstreams: cell_configs
                .into_iter()
                .map(|(name, config)| (name, config.into()))
                .collect(),
        }
    }
}

/// Maps locales to their cells (which map to upstreams)
#[derive(Clone, Debug)]
pub struct Locale {
    /// Mapping from locale to cells
    locale_to_cells: HashMap<String, Cells>,
}

impl Locale {
    /// Build locale mappings from configuration
    pub fn new(locales: HashMap<String, HashMap<String, CellConfig>>) -> Self {
        // Build locale -> cells mapping
        let locale_to_cells = locales
            .into_iter()
            .map(|(locale, cells)| {
                let cells = Cells::from_config(cells);
                (locale, cells)
            })
            .collect();

        Self { locale_to_cells }
    }

    /// Get the cells for a specific locale
    pub fn get_cells(&self, locale: &str) -> Option<&Cells> {
        self.locale_to_cells.get(locale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_config(sentry_url: &str, relay_url: &str) -> CellConfig {
        CellConfig {
            sentry_url: Url::parse(sentry_url).unwrap(),
            relay_url: Url::parse(relay_url).unwrap(),
        }
    }

    #[test]
    fn test_locale() {
        let mut locales = HashMap::new();
        locales.insert(
            "us".to_string(),
            HashMap::from([
                (
                    "us1".to_string(),
                    cell_config("http://us1-sentry.example.com", "http://us1-relay.example.com"),
                ),
                (
                    "us2".to_string(),
                    cell_config("http://us2-sentry.example.com", "http://us2-relay.example.com"),
                ),
            ]),
        );
        locales.insert(
            "de".to_string(),
            HashMap::from([(
                "de".to_string(),
                cell_config("http://de-sentry.example.com", "http://de-relay.example.com"),
            )]),
        );

        let locale = Locale::new(locales);

        // Verify US locale has 2 cells
        let us_cells = locale.get_cells("us").unwrap();
        assert_eq!(us_cells.cell_to_upstreams.len(), 2);
        assert!(us_cells.cell_to_upstreams.contains_key("us1"));
        assert!(us_cells.cell_to_upstreams.contains_key("us2"));

        // Verify DE locale has 1 cell
        let de_cells = locale.get_cells("de").unwrap();
        assert_eq!(de_cells.cell_to_upstreams.len(), 1);
        assert!(de_cells.cell_to_upstreams.contains_key("de"));

        // Verify unknown locale returns None
        assert!(locale.get_cells("unknown").is_none());
    }
}

