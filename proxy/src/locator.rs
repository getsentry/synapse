use crate::config::{Locator as LocatorConfig, LocatorType};
use crate::errors::ProxyError;
use http::StatusCode;
use locator::config::LocatorDataType;
use locator::get_provider;
use locator::locator::{Locator as LocatorService, LocatorError};
use std::collections::HashMap;

#[derive(Clone)]
pub struct Locator(LocatorInner);

impl Locator {
    pub fn new_from_config(config: LocatorConfig) -> Self {
        match config.r#type {
            LocatorType::InProcess {
                control_plane,
                backup_route_store,
                locality_to_default_cell,
            } => {
                let provider = get_provider(backup_route_store.r#type);
                Locator(LocatorInner::InProcess(LocatorService::new(
                    LocatorDataType::Organization,
                    control_plane.url,
                    provider,
                    locality_to_default_cell,
                )))
            }
            LocatorType::Url { url } => Locator(LocatorInner::Url(Url::new(url))),
        }
    }

    pub async fn lookup(&self, org_id: &str, locality: Option<&str>) -> Result<String, ProxyError> {
        match &self.0 {
            LocatorInner::InProcess(l) => Ok(l.lookup(org_id, locality).await?),
            LocatorInner::Url(url) => Ok(url.lookup(org_id, locality).await?),
        }
    }

    pub fn is_ready(&self) -> bool {
        match &self.0 {
            LocatorInner::InProcess(l) => l.is_ready(),
            LocatorInner::Url(url) => url.is_ready(),
        }
    }

    pub async fn shutdown(&self) {
        match &self.0 {
            LocatorInner::InProcess(l) => l.shutdown().await,
            LocatorInner::Url(url) => url.shutdown(),
        }
    }
}

#[cfg(test)]
use locator::backup_routes::BackupRouteProvider;
#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
impl Locator {
    pub fn new_in_process(
        control_plane_url: String,
        backup_provider: Arc<dyn BackupRouteProvider + 'static>,
        locality_to_default_cell: Option<HashMap<String, String>>,
    ) -> Self {
        Locator(LocatorInner::InProcess(LocatorService::new(
            LocatorDataType::Organization,
            control_plane_url,
            backup_provider,
            locality_to_default_cell,
        )))
    }
}

#[derive(Clone)]
enum LocatorInner {
    InProcess(LocatorService),
    Url(Url),
}

#[derive(serde::Deserialize)]
struct LocatorApiResponse {
    cell: String,
}

#[derive(Clone)]
struct Url {
    client: reqwest::Client,
    url: String,
}

impl Url {
    pub fn new(url: String) -> Self {
        Url {
            client: reqwest::Client::new(),
            url,
        }
    }

    async fn lookup(&self, org_id: &str, locality: Option<&str>) -> Result<String, ProxyError> {
        let mut query_params = HashMap::new();
        query_params.insert("id", org_id);

        if let Some(loc) = locality {
            query_params.insert("locality", loc);
        }

        let response = self
            .client
            .get(&self.url)
            .query(&query_params)
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => Ok(response.json::<LocatorApiResponse>().await?.cell),
            StatusCode::NOT_FOUND => Err(ProxyError::LocatorError(LocatorError::NoCell)),
            StatusCode::SERVICE_UNAVAILABLE => {
                Err(ProxyError::LocatorError(LocatorError::NotReady))
            }
            _ => Err(ProxyError::LocatorError(LocatorError::InternalError)),
        }
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn shutdown(&self) {}
}
