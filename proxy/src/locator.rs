use crate::config::{Locator as LocatorConfig, LocatorType};
use crate::errors::ProxyError;
use locator::get_provider;
use locator::locator::Locator as LocatorService;

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
                    control_plane.url,
                    provider,
                    locality_to_default_cell,
                )))
            }
            LocatorType::Url { .. } => Locator(LocatorInner::Url(Url {})),
        }
    }

    pub async fn lookup(&self, org_id: &str, locality: Option<&str>) -> Result<String, ProxyError> {
        match &self.0 {
            LocatorInner::InProcess(l) => Ok(l.lookup(org_id, locality).await?),
            LocatorInner::Url(url) => url.lookup(org_id, locality),
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
use std::collections::HashMap;
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
            control_plane_url,
            backup_provider,
            locality_to_default_cell,
        )))
    }
}

#[derive(Clone)]
enum LocatorInner {
    InProcess(LocatorService),
    #[allow(dead_code)]
    Url(Url),
}

#[derive(Clone)]
struct Url {}

impl Url {
    fn lookup(&self, _org_id: &str, _locality: Option<&str>) -> Result<String, ProxyError> {
        todo!();
    }

    fn is_ready(&self) -> bool {
        todo!();
    }

    fn shutdown(&self) {
        todo!();
    }
}
