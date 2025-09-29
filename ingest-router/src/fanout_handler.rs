use crate::handler::{HandlerResult, RequestContext, RouteHandler};
use async_trait::async_trait;
use hyper::{Request, body::Incoming};
use std::time::Duration;

/// Handler that fans out requests to multiple downstream services and aggregates responses
pub struct FanOutHandler {
    _client_placeholder: (),
    _targets: Vec<String>,
    timeout_duration: Duration,
}

impl FanOutHandler {
    pub fn new(targets: Vec<String>) -> Self {
        Self {
            _client_placeholder: (),
            _targets: targets,
            timeout_duration: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_duration = Duration::from_secs(timeout_secs);
        self
    }
}

#[async_trait]
impl RouteHandler for FanOutHandler {
    async fn handle(&self, _request: Request<Incoming>, _context: RequestContext) -> HandlerResult {
        unimplemented!()
    }

    fn name(&self) -> &'static str {
        "fan_out"
    }
}
