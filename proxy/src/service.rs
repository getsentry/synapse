use hyper::service::Service as HyperService;

use crate::admin_service::AdminService;
use crate::proxy_service::ProxyService;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;
use hyper::{Request, Response};
use std::future::Future;
use std::pin::Pin;

pub enum ServiceType {
    Proxy(Box<ProxyService<Incoming>>),
    Admin(Box<AdminService>),
}

impl HyperService<Request<Incoming>> for ServiceType {
    type Response = Response<BoxBody<Bytes, hyper::Error>>;
    type Error = hyper::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        match self {
            ServiceType::Proxy(proxy) => proxy.call(req),
            ServiceType::Admin(admin) => admin.call(req),
        }
    }
}
