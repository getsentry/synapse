use crate::errors::IngestRouterError;
use crate::locale::Cells;
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

pub type CellId = String;

/// Handler for endpoints that split requests across cells and merge results
///
/// The handler implements endpoint-specific logic:
/// - How to split a request into per-cell requests
/// - How to merge results from multiple cells
/// ```
#[async_trait]
pub trait Handler<Req, Res>: Send + Sync
where
    Req: Serialize + DeserializeOwned + Send,
    Res: Serialize + DeserializeOwned + Send,
{
    /// Metadata that flows from split_requests to merge_results
    ///
    /// This allows passing data from the split phase to the merge phase.
    /// Some use cases:
    /// - Pending keys that couldn't be routed (e.g., `Vec<PublicKey>`)
    type SplitMetadata: Send;

    /// Split one request into multiple per-cell requests
    ///
    /// This method routes the request data to appropriate cells and builds
    /// per-cell requests that will be sent to upstreams.
    async fn split_requests(
        &self,
        request: Req,
        cells: &Cells,
    ) -> Result<(Vec<(CellId, Req)>, Self::SplitMetadata), IngestRouterError>;

    /// Merge results from multiple cells into a single response
    ///
    /// This method combines responses from successful cells, handles failures,
    /// and incorporates metadata from the split phase.
    ///
    fn merge_results(
        &self,
        results: Vec<Result<(CellId, Res), (CellId, IngestRouterError)>>,
        metadata: Self::SplitMetadata,
    ) -> Res;
}


