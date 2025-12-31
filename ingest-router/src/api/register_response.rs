use crate::errors::IngestRouterError;
use crate::handler::{CellId, Handler, SplitMetadata};
use crate::locale::Cells;
use async_trait::async_trait;
use hyper::body::Bytes;
use hyper::{Request, Response};

/// This endpoint returns success if any one upstream is available.
/// Synapse should continue to operate even if one cell is down.
pub struct RegisterResponse;

#[async_trait]
impl Handler for RegisterResponse {
    async fn split_request(
        &self,
        _request: Request<Bytes>,
        _cells: &Cells,
    ) -> Result<(Vec<(CellId, Request<Bytes>)>, SplitMetadata), IngestRouterError> {
        unimplemented!();
    }

    async fn merge_responses(
        &self,
        _responses: Vec<(CellId, Result<Response<Bytes>, IngestRouterError>)>,
        _metadata: SplitMetadata,
    ) -> Response<Bytes> {
        unimplemented!();
    }
}
