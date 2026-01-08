use crate::http::make_boxed_error_response;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::Service;
use hyper::{Request, Response, StatusCode};
use std::convert::Infallible;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

pub struct AdminService<F, E> {
    is_ready: F,
    _error: PhantomData<E>,
}

impl<F, E> AdminService<F, E>
where
    F: Fn() -> bool,
{
    pub fn new(is_ready: F) -> Self {
        Self {
            is_ready,
            _error: PhantomData,
        }
    }
}

impl<F, E> Service<Request<Incoming>> for AdminService<F, E>
where
    F: Fn() -> bool + Clone + Send + 'static,
    E: Send + 'static,
{
    type Response = Response<BoxBody<Bytes, Infallible>>;
    type Error = E;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let is_ready = (self.is_ready)();

        Box::pin(async move {
            let ok_body = || Full::new(Bytes::from("ok\n")).boxed();

            let res = match req.uri().path() {
                "/health" => Response::new(ok_body()),
                "/ready" => match is_ready {
                    true => Response::new(ok_body()),
                    false => make_boxed_error_response(StatusCode::SERVICE_UNAVAILABLE),
                },
                _ => make_boxed_error_response(StatusCode::NOT_FOUND),
            };
            Ok(res)
        })
    }
}
