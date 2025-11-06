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
        params: HashMap<String, &'a str>,
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

    fn cell_from_organization(&self, params: HashMap<String, &str>) -> Result<String, ProxyError> {
        let org = params
            .get("organization")
            .copied()
            .ok_or(ProxyError::ResolverError)?;

        self.locator.lookup(org, None)
    }

    fn cell_from_id(&self, params: HashMap<String, &str>) -> Result<String, ProxyError> {
        params
            .get("id")
            .ok_or(ProxyError::ResolverError)
            .map(|id| id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locator::backup_routes::{BackupRouteProvider, FilesystemRouteProvider};
    use locator::types::RouteData;
    use std::sync::Arc;

    fn get_mock_provider() -> (tempfile::TempDir, FilesystemRouteProvider) {
        let route_data = RouteData::from(
            HashMap::from([
                ("org_0".into(), "us1".into()),
                ("org_1".into(), "us1".into()),
                ("org_2".into(), "de".into()),
            ]),
            "cursor1".into(),
            HashMap::from([("us1".into(), "us".into()), ("de".into(), "de".into())]),
        );

        let dir = tempfile::tempdir().unwrap();
        let provider = FilesystemRouteProvider::new(dir.path().to_str().unwrap(), "backup.bin");
        provider.store(&route_data).unwrap();
        (dir, provider)
    }

    #[tokio::test]
    async fn test_resolve() {
        let (_dir, provider) = get_mock_provider();
        let locator = Locator::new_in_process(
            "http://control-plane-url".to_string(),
            Arc::new(provider),
            None,
        );

        // wait for locator to become ready
        for _ in 0..5 {
            if locator.is_ready() {
                break;
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }

        let resolvers = Resolvers::try_new(locator).unwrap();
        let mut cell_to_upstream = HashMap::new();
        cell_to_upstream.insert("us1".to_string(), "upstream1".to_string());

        // Valid cell id
        let mut params = HashMap::new();
        params.insert("id".to_string(), "us1");
        let result = resolvers
            .resolve("cell_from_id", &cell_to_upstream, params.clone())
            .unwrap();
        assert_eq!(result, "upstream1");

        // Invalid cell id
        let mut invalid_params = HashMap::new();
        invalid_params.insert("id".to_string(), "us999");

        let result = resolvers.resolve("cell_from_id", &cell_to_upstream, invalid_params);

        assert!(result.is_err());

        // valid org
        let mut org_params = HashMap::new();
        org_params.insert("organization".to_string(), "org_0");

        let result = resolvers
            .resolve("cell_from_organization", &cell_to_upstream, org_params)
            .unwrap();

        assert_eq!(result, "upstream1");

        // invalid org
        let mut invalid_org_params = HashMap::new();
        invalid_org_params.insert("organization".to_string(), "org_999");

        let result = resolvers.resolve(
            "cell_from_organization",
            &cell_to_upstream,
            invalid_org_params,
        );

        assert!(result.is_err());
    }
}
