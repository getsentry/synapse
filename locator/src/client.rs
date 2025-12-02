use crate::config::{BackupRouteStoreType, LocatorDataType};
use crate::get_provider;
use crate::locator::{Locator as LocatorService, LocatorError};
use http::StatusCode;
use std::collections::HashMap;

#[derive(thiserror::Error, Debug)]
pub enum ClientError {
    #[error("Locator error: {0}")]
    LocatorError(#[from] LocatorError),
    #[error("Backup route provider error: {0}")]
    BackupError(#[from] crate::backup_routes::BackupError),
    #[error("HTTP client error: {0}")]
    ReqwestError(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Configuration for creating a Locator client
pub struct LocatorConfig {
    pub locator_type: LocatorType,
    pub data_type: LocatorDataType,
}

pub enum LocatorType {
    InProcess {
        control_plane_url: String,
        backup_route_store_type: BackupRouteStoreType,
        locality_to_default_cell: Option<HashMap<String, String>>,
    },
    Url {
        url: String,
    },
}

/// A unified locator client that can work with either an in-process locator
/// or a remote locator via HTTP.
#[derive(Clone)]
pub struct Locator(LocatorInner);

impl Locator {
    pub async fn new(config: LocatorConfig) -> Result<Self, ClientError> {
        match config.locator_type {
            LocatorType::InProcess {
                control_plane_url,
                backup_route_store_type,
                locality_to_default_cell,
            } => {
                let provider = get_provider(backup_route_store_type).await?;
                Ok(Locator(LocatorInner::InProcess(LocatorService::new(
                    config.data_type,
                    control_plane_url,
                    provider,
                    locality_to_default_cell,
                ))))
            }
            LocatorType::Url { url } => Ok(Locator(LocatorInner::Url(HttpClient::new(url)))),
        }
    }

    /// Create a locator from an existing in-process LocatorService.
    /// This is useful when you need to provide a custom-configured service.
    pub fn from_in_process_service(service: LocatorService) -> Self {
        Locator(LocatorInner::InProcess(service))
    }

    pub async fn lookup(&self, id: &str, locality: Option<&str>) -> Result<String, ClientError> {
        match &self.0 {
            LocatorInner::InProcess(l) => Ok(l.lookup(id, locality).await?),
            LocatorInner::Url(client) => Ok(client.lookup(id, locality).await?),
        }
    }

    pub fn is_ready(&self) -> bool {
        match &self.0 {
            LocatorInner::InProcess(l) => l.is_ready(),
            LocatorInner::Url(client) => client.is_ready(),
        }
    }

    pub async fn shutdown(&self) {
        match &self.0 {
            LocatorInner::InProcess(l) => l.shutdown().await,
            LocatorInner::Url(client) => client.shutdown(),
        }
    }
}

#[derive(Clone)]
enum LocatorInner {
    InProcess(LocatorService),
    Url(HttpClient),
}

#[derive(serde::Deserialize)]
struct LocatorApiResponse {
    cell: String,
}

#[derive(Clone)]
struct HttpClient {
    client: reqwest::Client,
    url: String,
}

impl HttpClient {
    pub fn new(url: String) -> Self {
        HttpClient {
            client: reqwest::Client::new(),
            url,
        }
    }

    async fn lookup(&self, id: &str, locality: Option<&str>) -> Result<String, ClientError> {
        let mut query_params = HashMap::new();
        query_params.insert("id", id);

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
            StatusCode::NOT_FOUND => Err(ClientError::LocatorError(LocatorError::NoCell)),
            StatusCode::SERVICE_UNAVAILABLE => {
                Err(ClientError::LocatorError(LocatorError::NotReady))
            }
            _ => Err(ClientError::LocatorError(LocatorError::InternalError)),
        }
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn shutdown(&self) {}
}
