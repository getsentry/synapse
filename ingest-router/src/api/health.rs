use crate::errors::IngestRouterError;
use crate::handler::{CellId, Handler, HandlerBody, SplitMetadata};
use crate::locale::Cells;
use async_trait::async_trait;
use hyper::{Request, Response};

/// This endpoint returns success if any one upstream is available.
/// Synapse should continue to operate even if one cell is down.
pub struct HealthHandler {}

#[async_trait]
impl Handler for HealthHandler {
    async fn split_request(
        &self,
        _request: Request<HandlerBody>,
        _cells: &Cells,
    ) -> Result<(Vec<(CellId, Request<HandlerBody>)>, SplitMetadata), IngestRouterError> {
        unimplemented!();
    }

    async fn merge_responses(
        &self,
        _responses: Vec<(CellId, Result<Response<HandlerBody>, IngestRouterError>)>,
        _metadata: SplitMetadata,
    ) -> Response<HandlerBody> {
        unimplemented!();
    }
}
