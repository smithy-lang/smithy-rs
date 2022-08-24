use std::convert::Infallible;

use http::header::ToStrError;
use tower::{Layer, Service};

use crate::{
    body::BoxBody,
    extension::RuntimeErrorExtension,
    protocols::{AwsJson10, AwsJson11},
    response::IntoResponse,
    routing::{tiny_map::TinyMap, Route},
};

use super::Router;

pub enum Error {
    NotRootUrl,
    MethodDisallowed,
    MissingHeader,
    InvalidHeader(ToStrError),
    NotFound,
}

impl IntoResponse<AwsJson10> for Error {
    fn into_response(self) -> http::Response<BoxBody> {
        match self {
            Error::MethodDisallowed => super::method_disallowed(),
            _ => http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/x-amz-json-1.0")
                .extension(RuntimeErrorExtension::new(
                    super::UNKNOWN_OPERATION_EXCEPTION.to_string(),
                ))
                .body(crate::body::to_boxed(""))
                .expect("invalid HTTP response"),
        }
    }
}

impl IntoResponse<AwsJson11> for Error {
    fn into_response(self) -> http::Response<BoxBody> {
        match self {
            Error::MethodDisallowed => super::method_disallowed(),
            _ => http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/x-amz-json-1.1")
                .extension(RuntimeErrorExtension::new(
                    super::UNKNOWN_OPERATION_EXCEPTION.to_string(),
                ))
                .body(crate::body::to_boxed(""))
                .expect("invalid HTTP response"),
        }
    }
}

// This constant determines when the `TinyMap` implementation switches from being a `Vec` to a
// `HashMap`. This is chosen to be 15 as a result of the discussion around
// https://github.com/awslabs/smithy-rs/pull/1429#issuecomment-1147516546
const ROUTE_CUTOFF: usize = 15;

#[derive(Debug, Clone)]
pub struct AwsJsonRouter<S> {
    routes: TinyMap<String, S, ROUTE_CUTOFF>,
}

impl<S> AwsJsonRouter<S> {
    pub fn layer<L>(self, layer: L) -> AwsJsonRouter<L::Service>
    where
        L: Layer<S>,
    {
        AwsJsonRouter {
            routes: self
                .routes
                .into_iter()
                .map(|(key, route)| (key, layer.layer(route)))
                .collect(),
        }
    }

    pub fn boxed<B>(self) -> AwsJsonRouter<Route<B>>
    where
        S: Service<http::Request<B>, Response = http::Response<BoxBody>, Error = Infallible>,
        S: Send + Clone + 'static,
        S::Future: Send + 'static,
    {
        AwsJsonRouter {
            routes: self.routes.into_iter().map(|(key, s)| (key, Route::new(s))).collect(),
        }
    }
}

impl<B, S> Router<B> for AwsJsonRouter<S>
where
    S: Clone,
{
    type Service = S;
    type Error = Error;

    fn match_route(&self, request: &http::Request<B>) -> Result<S, Self::Error> {
        if request.uri() != "/" {
            return Err(Error::NotRootUrl);
        }

        if request.method() != http::Method::POST {
            return Err(Error::MethodDisallowed);
        }

        // Find the `x-amz-target` header.
        let target = request.headers().get("x-amz-target").ok_or(Error::MissingHeader)?;
        let target = target.to_str().map_err(Error::InvalidHeader)?;

        // Lookup in the `TinyMap` for a route for the target.
        let route = self.routes.get(target).ok_or(Error::NotFound)?;
        Ok(route.clone())
    }
}

impl<S> FromIterator<(String, S)> for AwsJsonRouter<S> {
    #[inline]
    fn from_iter<T: IntoIterator<Item = (String, S)>>(iter: T) -> Self {
        Self {
            routes: iter
                .into_iter()
                .map(|(svc, request_spec)| (svc, request_spec))
                .collect(),
        }
    }
}
