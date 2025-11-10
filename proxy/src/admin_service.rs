use crate::locator::Locator;
use crate::utils::make_error_response;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::Service as HyperService;
use hyper::{Request, Response, StatusCode};
use std::future::Future;
use std::pin::Pin;

pub struct AdminService {
    locator: Locator,
}

impl AdminService {
    pub fn new(locator: Locator) -> Self {
        Self { locator }
    }
}

impl HyperService<Request<Incoming>> for AdminService {
    type Response = Response<BoxBody<Bytes, hyper::Error>>;
    type Error = hyper::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let is_ready = self.locator.is_ready();

        Box::pin(async move {
            let res = match req.uri().path() {
                "/health" => {
                    Response::new(Full::new("ok\n".into()).map_err(|e| match e {}).boxed())
                }
                "/ready" => match is_ready {
                    true => Response::new(Full::new("ok\n".into()).map_err(|e| match e {}).boxed()),
                    false => make_error_response(StatusCode::SERVICE_UNAVAILABLE),
                },
                _ => make_error_response(StatusCode::NOT_FOUND),
            };
            Ok(res)
        })
    }
}
