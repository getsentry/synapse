use crate::handler::{HandlerResult, RequestContext, RouteHandler};
use async_trait::async_trait;
use hyper::{Request, body::Incoming};
use std::time::Duration;

/// Handler that forwards requests to a single downstream service
pub struct ForwardHandler {
    _client_placeholder: (),
    _target_urls: Vec<String>,
    timeout_duration: Duration,
}

impl ForwardHandler {
    pub fn new(target_urls: Vec<String>) -> Self {
        Self {
            _client_placeholder: (),
            _target_urls: target_urls,
            timeout_duration: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_duration = Duration::from_secs(timeout_secs);
        self
    }
}

#[async_trait]
impl RouteHandler for ForwardHandler {
    async fn handle(&self, _request: Request<Incoming>, _context: RequestContext) -> HandlerResult {
        unimplemented!()
    }

    fn name(&self) -> &'static str {
        "forward"
    }
}
