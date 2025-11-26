//! Key routing logic for distributing requests across upstream cells.
//!
//! Current: Round-robin distribution (placeholder)
//! TODO: Replace with control plane locator service lookup

use crate::locale::{Cells, Upstream};
use std::collections::HashMap;

use super::protocol::ProjectConfigsRequest;

/// A request split for a specific upstream cell.
#[derive(Clone)]
pub struct SplitRequest {
    pub cell_name: String,
    pub upstream: Upstream,
    pub request: ProjectConfigsRequest,
}

/// Routes public keys to their owning cells.
pub struct PublicKeyRouter {
    // TODO: Add locator service in future PR
}

impl PublicKeyRouter {
    pub fn new() -> Self {
        Self {}
    }

    /// Splits request across cells using round-robin distribution.
    ///
    /// TODO: Replace with locator service lookup per key.
    pub fn split(&self, request: &ProjectConfigsRequest, cells: &Cells) -> Vec<SplitRequest> {
        if cells.cell_list.is_empty() {
            return Vec::new();
        }

        let mut split: HashMap<String, Vec<String>> = HashMap::new();

        // TODO: Replace with locator service lookup
        for (index, public_key) in request.public_keys.iter().enumerate() {
            let cell_name = &cells.cell_list[index % cells.cell_list.len()];
            split
                .entry(cell_name.clone())
                .or_default()
                .push(public_key.clone());
        }

        split
            .into_iter()
            .map(|(cell_name, public_keys)| {
                let upstream = cells
                    .cell_to_upstreams
                    .get(&cell_name)
                    .expect("Cell name in list must exist in HashMap");

                SplitRequest {
                    cell_name,
                    upstream: upstream.clone(),
                    request: ProjectConfigsRequest {
                        public_keys,
                        extra_fields: request.extra_fields.clone(),
                    },
                }
            })
            .collect()
    }
}

impl Default for PublicKeyRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;
    use crate::locale::Locales;
    use std::collections::HashMap;
    use url::Url;

    #[test]
    fn test_extra_fields_passthrough() {
        let splitter = PublicKeyRouter::new();

        let locales = HashMap::from([(
            "us".to_string(),
            vec![CellConfig {
                name: "us1".to_string(),
                sentry_url: Url::parse("http://us1:8080").unwrap(),
                relay_url: Url::parse("http://us1:8090").unwrap(),
            }],
        )]);

        let locales_obj = Locales::new(locales);
        let cells = locales_obj.get_cells("us").unwrap();

        let mut extra = HashMap::new();
        extra.insert("global".to_string(), serde_json::json!(true));

        let request = ProjectConfigsRequest {
            public_keys: vec!["key1".to_string()],
            extra_fields: extra.clone(),
        };

        let splits = splitter.split(&request, cells);

        assert_eq!(splits[0].request.extra_fields, extra);
    }
}
