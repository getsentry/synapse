use crate::errors::IngestRouterError;
use crate::handler::{CellId, ExecutionMode, Handler, SplitMetadata};
use crate::locale::Cells;
use async_trait::async_trait;
use hyper::body::Bytes;
use hyper::{Request, Response};

/// Handler for the public keys endpoint.
///
/// `POST /api/0/relays/publickeys/`
///
///
/// Example request:
/// ```json
/// {
///   "relay_ids": ["key1", "key2"]
/// }
/// ```
///
/// Example response:
/// (key2 is not found in the upstreams)
///
/// ```json
/// {
///   "public_keys": {
///     "key1": "abc123...",
///     "key2": null
///   },
///   "relays": {
///     "key1": {
///       "publicKey": "abc123...",
///       "internal": false
///     },
///     "key2": null
///   }
/// }
/// ```
pub struct PublicKeysHandler;

#[async_trait]
impl Handler for PublicKeysHandler {
    fn name(&self) -> &'static str {
        "PublicKeysHandler"
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }

    async fn split_request(
        &self,
        _request: Request<Bytes>,
        _cells: &Cells,
    ) -> Result<(Vec<(CellId, Request<Bytes>)>, SplitMetadata), IngestRouterError> {
        unimplemented!()
    }

    async fn merge_responses(
        &self,
        _responses: Vec<(CellId, Result<Response<Bytes>, IngestRouterError>)>,
        _metadata: SplitMetadata,
    ) -> Response<Bytes> {
        unimplemented!()
    }
}
