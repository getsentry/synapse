use crate::errors::IngestRouterError;
use crate::locale::Cells;
use async_trait::async_trait;
use hyper::body::Bytes;
use hyper::{Request, Response};
use std::any::Any;

pub type CellId = String;
pub type SplitMetadata = Box<dyn Any + Send>;

pub enum ExecutionMode {
    // Requests are fanned out and executed in parallel across cells
    Parallel,
    // Requests are executed sequentially across cells in priority order
    // Subsequent requests are skipped if an earlier request succeeds
    Failover,
}

/// Handler for endpoints that split requests across cells and merge results
///
/// The handler implements endpoint-specific logic:
/// - How to split a request into per-cell requests
/// - How to merge results from multiple cells
#[async_trait]
pub trait Handler: Send + Sync {
    fn name(&self) -> &'static str;

    fn execution_mode(&self) -> ExecutionMode;

    /// Split one request into multiple per-cell requests
    ///
    /// This method routes the request data to appropriate cells and builds
    /// per-cell requests that will be sent to upstreams.
    async fn split_request(
        &self,
        request: Request<Bytes>,
        cells: &Cells,
    ) -> Result<(Vec<(CellId, Request<Bytes>)>, SplitMetadata), IngestRouterError>;

    /// Merge results from multiple cells into a single response
    ///
    /// This method combines responses from successful cells, handles failures,
    /// and incorporates metadata from the split phase.
    async fn merge_responses(
        &self,
        responses: Vec<(CellId, Result<Response<Bytes>, IngestRouterError>)>,
        metadata: SplitMetadata,
    ) -> Response<Bytes>;
}
