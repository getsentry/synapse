use async_trait::async_trait;
use hyper::{Request, Response, body::Incoming};
use std::collections::HashMap;
use std::error::Error;

/// Result type for route handlers
pub type HandlerResult = Result<Response<String>, Box<dyn Error + Send + Sync>>;

/// Context containing request metadata like URI, Method, Headers etc.
/// This will be used to reach the downstream destination.
#[derive(Debug)]
pub struct RequestContext {
    // TODO: Need to provide context of HTTP method, headers, URI etc.
}

/// Core trait that all route handlers must implement
#[async_trait]
pub trait RouteHandler: Send + Sync {
    /// Handle an incoming HTTP request
    async fn handle(&self, request: Request<Incoming>, context: RequestContext) -> HandlerResult;

    /// Handler name for registration and debugging
    fn name(&self) -> &'static str;
}

/// Registry for managing and looking up handlers by name
pub struct HandlerRegistry {
    handlers: HashMap<String, Box<dyn RouteHandler>>,
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a handler with a given name
    pub fn register(&mut self, name: String, handler: Box<dyn RouteHandler>) {
        self.handlers.insert(name, handler);
    }

    /// Get a handler by name
    pub fn get(&self, name: &str) -> Option<&dyn RouteHandler> {
        self.handlers.get(name).map(|h| h.as_ref())
    }

    /// List all registered handler names
    pub fn list_handlers(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }
}
