//! Locality-based Routing Infrastructure
//!
//! Maps localities to cells and cells to upstreams for request routing.
//!
//! # Model
//!
//! The routing system uses a two-level hierarchy:
//!
//! 1. **Locality → Cells**: Each locality (e.g., "us", "de") maps to cell names
//! 2. **Cell → Upstream**: Each cell name maps to an `Upstream` with URLs
//!
//! ## Example
//!
//! ```text
//! Locality "us" → Cells ["us-1", "us-2"]
//!   ├─ "us-1" → Upstream {
//!   │    relay_url: "http://us1-relay.example.com",
//!   │    sentry_url: "http://us1-sentry.example.com"
//!   │  }
//!   └─ "us-2" → Upstream { ... }
//! ```
//!
//! `Localties` is built at startup from configuration and remains immutable
//! during request processing.

use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;
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
#[derive(Debug)]
struct CellsInner {
    locality: String,
    /// Map of cell_id to upstream, preserving insertion order (first = highest priority)
    cells: IndexMap<String, Upstream>,
}

#[derive(Clone, Debug)]
pub struct Cells {
    inner: Arc<CellsInner>,
}

impl Cells {
    /// Build cells from cell configurations
    fn from_config(locality: String, cell_configs: Vec<CellConfig>) -> Self {
        let cells: IndexMap<String, Upstream> = cell_configs
            .into_iter()
            .map(|config| {
                let id = config.id.clone();
                let upstream = Upstream::from(config);
                (id, upstream)
            })
            .collect();

        Self {
            inner: Arc::new(CellsInner { locality, cells }),
        }
    }

    pub fn locality(&self) -> &str {
        &self.inner.locality
    }

    /// Returns order list of cell ids
    pub fn cell_list(&self) -> impl Iterator<Item = &String> {
        self.inner.cells.keys()
    }

    /// Get upstream for a cell_id, or None if not found
    pub fn get_upstream(&self, cell_id: &str) -> Option<&Upstream> {
        self.inner.cells.get(cell_id)
    }

    /// Check if a cell_id exists
    pub fn contains_cell(&self, cell_id: &str) -> bool {
        self.inner.cells.contains_key(cell_id)
    }
}

/// Maps localities to their cells (which map to upstreams)
pub struct Localities {
    /// Mapping from locality to cells
    locality_to_cells: HashMap<String, Cells>,
}

impl Localities {
    /// Build locality mappings from configuration
    pub fn new(localities: HashMap<String, Vec<CellConfig>>) -> Self {
        // Build locality -> cells mapping
        let locality_to_cells = localities
            .into_iter()
            .map(|(locality, cells_config)| {
                let cells = Cells::from_config(locality.clone(), cells_config);
                (locality, cells)
            })
            .collect();

        Self { locality_to_cells }
    }

    /// Get the cells for a specific locality
    pub fn get_cells(&self, locality: &str) -> Option<Cells> {
        self.locality_to_cells.get(locality).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_config(id: &str, sentry_url: &str, relay_url: &str) -> CellConfig {
        CellConfig {
            id: id.to_string(),
            sentry_url: Url::parse(sentry_url).unwrap(),
            relay_url: Url::parse(relay_url).unwrap(),
        }
    }

    #[test]
    fn test_localities() {
        let mut localities_config = HashMap::new();
        localities_config.insert(
            "us".to_string(),
            vec![
                cell_config(
                    "us1",
                    "http://us1-sentry.example.com",
                    "http://us1-relay.example.com",
                ),
                cell_config(
                    "us2",
                    "http://us2-sentry.example.com",
                    "http://us2-relay.example.com",
                ),
            ],
        );
        localities_config.insert(
            "de".to_string(),
            vec![cell_config(
                "de1",
                "http://de-sentry.example.com",
                "http://de-relay.example.com",
            )],
        );

        let localities = Localities::new(localities_config);

        // Verify US locality has 2 cells
        let us_cells = localities.get_cells("us").unwrap();
        let cell_list: Vec<_> = us_cells.cell_list().collect();
        assert_eq!(cell_list.len(), 2);
        assert!(us_cells.contains_cell("us1"));
        assert!(us_cells.contains_cell("us2"));
        assert!(us_cells.get_upstream("us1").is_some());
        assert!(us_cells.get_upstream("us2").is_some());
        // Verify priority order
        assert_eq!(cell_list[0], "us1");
        assert_eq!(cell_list[1], "us2");

        // Verify DE locality has 1 cell
        let de_cells = localities.get_cells("de").unwrap();
        let cell_list: Vec<_> = de_cells.cell_list().collect();
        assert_eq!(cell_list.len(), 1);
        assert!(de_cells.contains_cell("de1"));
        assert!(de_cells.get_upstream("de1").is_some());
        assert_eq!(cell_list[0], "de1");

        // Verify unknown locality returns None
        assert!(localities.get_cells("unknown").is_none());
    }
}
