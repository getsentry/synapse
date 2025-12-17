use std::io;

#[derive(thiserror::Error, Debug)]
pub enum ProxyError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("route configuration error: {0}")]
    InvalidRoute(String),
    #[error("upstream configuration error")]
    InvalidUpstream,
    #[error("invalid URI: {0}")]
    InvalidUri(#[from] http::uri::InvalidUri),
    #[error("could not resolve route")]
    ResolverError,
    #[error("locator reqwest error: {0}")]
    ReqwestError(#[from] reqwest::Error),
    #[error("hyper error: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("backup route provider error: {0}")]
    BackupError(#[from] locator::backup_routes::BackupError),
    #[error("locator client error: {0}")]
    LocatorClientError(#[from] locator::client::ClientError),
}
