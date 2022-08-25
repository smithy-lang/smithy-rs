/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;

use thiserror::Error;
use tower::{Layer, Service};

use crate::{
    body::BoxBody,
    extension::RuntimeErrorExtension,
    protocols::{AwsRestJson1, AwsRestXml},
    response::IntoResponse,
    routing::{
        request_spec::{Match, RequestSpec},
        Route,
    },
};

use super::Router;

/// An AWS REST routing error.
#[derive(Debug, Error)]
pub enum Error {
    /// Operation not found.
    #[error("operation not found")]
    NotFound,
    /// Method was not allowed.
    #[error("method was not allowed")]
    MethodNotAllowed,
}

impl IntoResponse<AwsRestJson1> for Error {
    fn into_response(self) -> http::Response<BoxBody> {
        match self {
            Error::NotFound => http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .header("X-Amzn-Errortype", super::UNKNOWN_OPERATION_EXCEPTION)
                .extension(RuntimeErrorExtension::new(
                    super::UNKNOWN_OPERATION_EXCEPTION.to_string(),
                ))
                .body(crate::body::to_boxed("{}"))
                .expect("invalid HTTP response"),
            Error::MethodNotAllowed => super::method_disallowed(),
        }
    }
}

impl IntoResponse<AwsRestXml> for Error {
    fn into_response(self) -> http::Response<BoxBody> {
        match self {
            Error::NotFound => http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/xml")
                .extension(RuntimeErrorExtension::new(
                    super::UNKNOWN_OPERATION_EXCEPTION.to_string(),
                ))
                .body(crate::body::to_boxed(""))
                .expect("invalid HTTP response"),
            Error::MethodNotAllowed => super::method_disallowed(),
        }
    }
}

/// A [`Router`] supporting [`AWS REST JSON 1.0`] and [`AWS REST XML`] protocols.
///
/// [AWS REST JSON 1.0]: https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restjson1-protocol.html
/// [AWS REST XML]: https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restxml-protocol.html
#[derive(Debug, Clone)]
pub struct RestRouter<S> {
    routes: Vec<(S, RequestSpec)>,
}

impl<S> RestRouter<S> {
    /// Applies a [`Layer`] uniformly to all routes.
    pub fn layer<L>(self, layer: L) -> RestRouter<L::Service>
    where
        L: Layer<S>,
    {
        RestRouter {
            routes: self
                .routes
                .into_iter()
                .map(|(route, request_spec)| (layer.layer(route), request_spec))
                .collect(),
        }
    }

    /// Applies type erasure to the inner route using [`Route::new`].
    pub fn boxed<B>(self) -> RestRouter<Route<B>>
    where
        S: Service<http::Request<B>, Response = http::Response<BoxBody>, Error = Infallible>,
        S: Send + Clone + 'static,
        S::Future: Send + 'static,
    {
        RestRouter {
            routes: self.routes.into_iter().map(|(s, spec)| (Route::new(s), spec)).collect(),
        }
    }
}

impl<B, S> Router<B> for RestRouter<S>
where
    S: Clone,
{
    type Service = S;
    type Error = Error;

    fn match_route(&self, request: &http::Request<B>) -> Result<S, Self::Error> {
        let mut method_allowed = true;

        for (route, request_spec) in &self.routes {
            match request_spec.matches(request) {
                // Match found.
                Match::Yes => return Ok(route.clone()),
                // Match found, but method disallowed.
                Match::MethodNotAllowed => method_allowed = false,
                // Continue looping to see if another route matches.
                Match::No => continue,
            }
        }

        if method_allowed {
            Err(Error::NotFound)
        } else {
            Err(Error::MethodNotAllowed)
        }
    }
}

impl<S> FromIterator<(S, RequestSpec)> for RestRouter<S> {
    #[inline]
    fn from_iter<T: IntoIterator<Item = (S, RequestSpec)>>(iter: T) -> Self {
        let mut routes: Vec<(S, RequestSpec)> = iter
            .into_iter()
            .map(|(svc, request_spec)| (svc, request_spec))
            .collect();

        // Sort them once by specificity, with the more specific routes sorted before the less
        // specific ones, so that when routing a request we can simply iterate through the routes
        // and pick the first one that matches.
        routes.sort_by_key(|(_route, request_spec)| std::cmp::Reverse(request_spec.rank()));

        Self { routes }
    }
}
