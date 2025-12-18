use crate::errors::IngestRouterError;
use crate::locale::Cells;
use async_trait::async_trait;
use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper::{Request, Response};
use std::any::Any;

pub type CellId = String;
pub type HandlerBody = BoxBody<Bytes, IngestRouterError>;
pub type SplitMetadata = Box<dyn Any + Send>;

/// Handler for endpoints that split requests across cells and merge results
///
/// The handler implements endpoint-specific logic:
/// - How to split a request into per-cell requests
/// - How to merge results from multiple cells
#[async_trait]
pub trait Handler: Send + Sync {
    /// Returns the type name of this handler for test assertions
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Split one request into multiple per-cell requests
    ///
    /// This method routes the request data to appropriate cells and builds
    /// per-cell requests that will be sent to upstreams.
    async fn split_request(
        &self,
        request: Request<HandlerBody>,
        cells: &Cells,
    ) -> Result<(Vec<(CellId, Request<HandlerBody>)>, SplitMetadata), IngestRouterError>;

    /// Merge results from multiple cells into a single response
    ///
    /// This method combines responses from successful cells, handles failures,
    /// and incorporates metadata from the split phase.
    fn merge_responses(
        &self,
        responses: Vec<(CellId, Result<Response<HandlerBody>, IngestRouterError>)>,
        metadata: SplitMetadata,
    ) -> Response<HandlerBody>;
}
