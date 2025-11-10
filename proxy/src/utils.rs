use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Response, StatusCode};

pub fn make_error_response(status_code: StatusCode) -> Response<BoxBody<Bytes, hyper::Error>> {
    let message = status_code
        .canonical_reason()
        .unwrap_or("an error occurred");

    let mut response = Response::new(Full::new(message.into()).map_err(|e| match e {}).boxed());
    *response.status_mut() = status_code;
    response
}
