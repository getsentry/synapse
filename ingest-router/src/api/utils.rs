use crate::errors::IngestRouterError;
use http::Version;
use hyper::body::Bytes;
use hyper::header::HeaderMap;
use hyper::header::{CONTENT_LENGTH, TRANSFER_ENCODING};
use serde::Serialize;
use serde::de::DeserializeOwned;
use shared::http::filter_hop_by_hop;

/// Deserializes a JSON body into the specified type.
pub fn deserialize_body<T: DeserializeOwned>(body: Bytes) -> Result<T, IngestRouterError> {
    serde_json::from_slice(&body).map_err(|e| IngestRouterError::RequestBodyError(e.to_string()))
}

/// Serializes a value to a JSON body.
pub fn serialize_to_body<T: Serialize>(value: &T) -> Result<Bytes, IngestRouterError> {
    serde_json::to_vec(value)
        .map(Bytes::from)
        .map_err(|e| IngestRouterError::RequestBodyError(e.to_string()))
}

/// Common header normalization for all requests and responses.
pub fn normalize_headers(headers: &mut HeaderMap, version: Version) -> &mut HeaderMap {
    filter_hop_by_hop(headers, version);
    headers.remove(CONTENT_LENGTH);
    headers.remove(TRANSFER_ENCODING);

    headers
}
