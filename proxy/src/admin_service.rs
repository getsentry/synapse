use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::Service as HyperService;
use hyper::{Request, Response, StatusCode};
use std::future::Future;
use std::pin::Pin;
use crate::locator::Locator;

pub struct AdminService {
    locator: Locator,
}

impl AdminService {
    pub fn new(locator: Locator) -> Self {
        Self {
            locator,
        }
    }
}

impl HyperService<Request<Incoming>> for AdminService {
    type Response = Response<BoxBody<Bytes, hyper::Error>>;
    type Error = hyper::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        Box::pin(async move {
            let res = match req.uri().path() {
                "/health" => {
                    Response::new(Full::new("ok\n".into()).map_err(|e| match e {}).boxed())
                }
                "/ready" => {
                    unimplemented!();
                }
                _ => Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(
                        Full::new("not found\n".into())
                            .map_err(|e| match e {})
                            .boxed(),
                    )
                    .unwrap(),
            };
            Ok(res)
        })
    }
}
