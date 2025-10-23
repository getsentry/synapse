use crate::errors::ProxyError;
use crate::locator::Locator;
use std::collections::HashMap;

pub struct Resolvers {
    locator: Locator,
}

impl Resolvers {
    pub fn try_new(locator: Locator) -> Result<Resolvers, ProxyError> {
        // TODO: add validation for route configurations here to ensure any invalid route definitions
        // are caught on startup.

        Ok(Resolvers { locator })
    }

    pub fn resolve<'a>(
        &self,
        resolver: &str,
        cell_to_upstream: &'a HashMap<String, String>,
        params: HashMap<&'a str, &'a str>,
    ) -> Result<&'a str, ProxyError> {
        // Resolve the upstream based on the resolver name and parameters
        // Return the upstream name or an error if resolution fails
        let cell = match resolver {
            "cell_from_organization" => self.cell_from_organization(params),
            "cell_from_id" => self.cell_from_id(params),
            _ => Err(ProxyError::InvalidResolver)?,
        }?;
        cell_to_upstream
            .get(&cell)
            .map(|s| s.as_str())
            .ok_or(ProxyError::ResolverError)
    }

    fn cell_from_organization<'a>(
        &self,
        params: HashMap<&'a str, &'a str>,
    ) -> Result<String, ProxyError> {
        let org = params
            .get("organization")
            .copied()
            .ok_or(ProxyError::ResolverError)?;

        self.locator.lookup(org, None)
    }

    fn cell_from_id<'a>(&self, params: HashMap<&'a str, &'a str>) -> Result<String, ProxyError> {
        params
            .get("id")
            .copied()
            .ok_or(ProxyError::ResolverError)
            .map(|id| id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Locator as LocatorConfig, LocatorType};
    use crate::locator::Locator;
    use locator::config::{BackupRouteStore, BackupRouteStoreType, ControlPlane};

    #[tokio::test]
    async fn test_resolve() {
        let locator_config = LocatorConfig {
            r#type: LocatorType::InProcess {
                control_plane: ControlPlane {
                    url: "http://localhost:8080".to_string(),
                },
                backup_route_store: BackupRouteStore {
                    r#type: BackupRouteStoreType::None,
                },
            },
        };

        let locator = Locator::new(locator_config.clone());

        let resolvers = Resolvers::try_new(locator).unwrap();
        let mut cell_to_upstream = HashMap::new();
        cell_to_upstream.insert("cell1".to_string(), "upstream1".to_string());

        // Valid cell id
        let mut params = HashMap::new();
        params.insert("id", "cell1");
        let result = resolvers
            .resolve("cell_from_id", &cell_to_upstream, params.clone())
            .unwrap();
        assert_eq!(result, "upstream1");

        // Invalid cell id
        let mut invalid_params = HashMap::new();
        invalid_params.insert("id", "cell2");

        let result = resolvers.resolve("cell_from_id", &cell_to_upstream, invalid_params);

        assert!(result.is_err());

        // resolve by organization
        let mut org_params = HashMap::new();
        org_params.insert("organization", "org1");
        // TODO: how to put this value in the locator
    }
}
