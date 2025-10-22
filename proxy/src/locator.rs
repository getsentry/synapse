use crate::config::{Locator as LocatorConfig, LocatorType};
use crate::errors::ProxyError;
use locator::get_provider;
use locator::locator::Locator as LocatorService;
use std::sync::Arc;

#[derive(Clone)]
pub struct Locator(LocatorInner);

impl Locator {
    pub fn new(config: LocatorConfig) -> Self {
        match config.r#type {
            LocatorType::InProcess { backup_route_store } => {
                let provider = get_provider(backup_route_store.r#type);
                Locator(LocatorInner::InProcess(LocatorService::new(provider)))
            }
            LocatorType::Url { .. } => todo!(),
        }
    }

    pub fn lookup(&self, org_id: &str, locality: Option<&str>) -> Result<Arc<String>, ProxyError> {
        match &self.0 {
            LocatorInner::InProcess(l) => Ok(l.lookup(org_id, locality)?.id),
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

#[derive(Clone)]
enum LocatorInner {
    InProcess(LocatorService),
    #[allow(dead_code)]
    Url(Url),
}

#[derive(Clone)]
struct Url {}

impl Url {
    fn lookup(&self, _org_id: &str, _locality: Option<&str>) -> Result<Arc<String>, ProxyError> {
        todo!();
    }

    fn is_ready(&self) -> bool {
        todo!();
    }
}
