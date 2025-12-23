use crate::errors::IngestRouterError;
use http::Version;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::header::HeaderMap;
use hyper::header::{CONTENT_LENGTH, TRANSFER_ENCODING};
use serde::Serialize;
use serde::de::DeserializeOwned;
use shared::http::filter_hop_by_hop;

pub type HandlerBody = BoxBody<Bytes, IngestRouterError>;

/// Deserializes a JSON request body into the specified type.
pub async fn deserialize_body<T: DeserializeOwned>(
    body: HandlerBody,
) -> Result<T, IngestRouterError> {
    let bytes = body.collect().await?.to_bytes();
    serde_json::from_slice(&bytes).map_err(|e| IngestRouterError::RequestBodyError(e.to_string()))
}

/// Serializes a value to a JSON body.
pub fn serialize_to_body<T: Serialize>(value: &T) -> Result<HandlerBody, IngestRouterError> {
    let bytes = serde_json::to_vec(value).map(Bytes::from)?;
    Ok(Full::new(bytes).map_err(|e| match e {}).boxed())
}

/// Common header normalization for all requests and responses.
pub fn normalize_headers(headers: &mut HeaderMap, version: Version) -> &mut HeaderMap {
    filter_hop_by_hop(headers, version);
    headers.remove(CONTENT_LENGTH);
    headers.remove(TRANSFER_ENCODING);

    headers
}
