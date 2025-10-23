use crate::config::{Locator as LocatorConfig, LocatorType};
use crate::errors::ProxyError;
use locator::get_provider;
use locator::locator::Locator as LocatorService;

#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
use locator::backup_routes::BackupRouteProvider;

#[derive(Clone)]
pub struct Locator(LocatorInner);

impl Locator {
    pub fn new_from_config(config: LocatorConfig) -> Self {
        match config.r#type {
            LocatorType::InProcess {
                control_plane,
                backup_route_store,
            } => {
                let provider = get_provider(backup_route_store.r#type);
                Locator(LocatorInner::InProcess(LocatorService::new(
                    control_plane.url,
                    provider,
                )))
            }
            LocatorType::Url { .. } => todo!(),
        }
    }

    pub fn lookup(&self, org_id: &str, locality: Option<&str>) -> Result<String, ProxyError> {
        match &self.0 {
            LocatorInner::InProcess(l) => Ok(l.lookup(org_id, locality)?),
            LocatorInner::Url(url) => url.lookup(org_id, locality),
        }
    }

    pub fn is_ready(&self) -> bool {
        match &self.0 {
            LocatorInner::InProcess(l) => l.is_ready(),
            LocatorInner::Url(url) => url.is_ready(),
        }
    }
}

#[cfg(test)]
impl Locator {
    pub fn new_in_process(
        control_plane_url: String,
        backup_provider: Arc<dyn BackupRouteProvider + 'static>,
    ) -> Self {
        Locator(LocatorInner::InProcess(LocatorService::new(
            control_plane_url,
            backup_provider,
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
}
