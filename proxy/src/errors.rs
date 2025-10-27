use std::io;

#[derive(thiserror::Error, Debug)]
pub enum ProxyError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("route configuration error")]
    InvalidRoute,
    #[error("upstream configuration error")]
    InvalidUpstream,
    #[error("invalid URI: {0}")]
    InvalidUri(#[from] http::uri::InvalidUri),
    #[error("unknown resolver")]
    InvalidResolver,
    #[error("could not resolve route")]
    ResolverError,
    #[error("locator error")]
    LocatorError(#[from] locator::locator::LocatorError),
}
