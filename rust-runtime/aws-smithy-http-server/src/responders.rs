use crate::model::*;
use axum::response::IntoResponse;
use http::Response;
use hyper::Body;

impl IntoResponse for HealthcheckOperationOutput {
    type Body = axum::body::Body;
    type BodyError = <Self::Body as axum::body::HttpBody>::Error;

    fn into_response(self) -> Response<Self::Body> {
        Response::builder().body(Body::from(String::from(""))).unwrap()
    }
}

impl IntoResponse for RegisterServiceOperationOutput {
    type Body = axum::body::Body;
    type BodyError = <Self::Body as axum::body::HttpBody>::Error;

    fn into_response(self) -> Response<Self::Body> {
        Response::builder().body(Body::from(String::from(""))).unwrap()
    }
}

// Operation error types.

impl IntoResponse for RegisterServiceError {
    type Body = axum::body::Body;
    type BodyError = <Self::Body as axum::body::HttpBody>::Error;

    fn into_response(self) -> Response<Self::Body> {
        Response::builder().body(Body::from(String::from(""))).unwrap()
    }
}
