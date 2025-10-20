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
}
