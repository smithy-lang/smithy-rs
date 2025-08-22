/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;

use tower::Layer;
use tower::Service;

#[cfg(feature = "http-02x")]
use crate::body::BoxBody;
use crate::routing::tiny_map::TinyMap;
#[cfg(feature = "http-02x")]
use crate::routing::Route;
#[cfg(feature = "http-02x")]
use crate::routing::Router;

#[cfg(feature = "http-02x")]
use http::header::ToStrError;
use thiserror::Error;

/// An AWS JSON routing error.
#[cfg(feature = "http-02x")]
#[derive(Debug, Error)]
pub enum Error {
    /// Relative URI was not "/".
    #[error("relative URI is not \"/\"")]
    NotRootUrl,
    /// Method was not `POST`.
    #[error("method not POST")]
    MethodNotAllowed,
    /// Missing the `x-amz-target` header.
    #[error("missing the \"x-amz-target\" header")]
    MissingHeader,
    /// Unable to parse header into UTF-8.
    #[error("failed to parse header: {0}")]
    #[cfg(feature = "http-02x")]
    InvalidHeader(ToStrError),
    /// Operation not found.
    #[error("operation not found")]
    NotFound,
}

// This constant determines when the `TinyMap` implementation switches from being a `Vec` to a
// `HashMap`. This is chosen to be 15 as a result of the discussion around
// https://github.com/smithy-lang/smithy-rs/pull/1429#issuecomment-1147516546
pub(crate) const ROUTE_CUTOFF: usize = 15;

/// A [`Router`] supporting [AWS JSON 1.0] and [AWS JSON 1.1] protocols.
///
/// [AWS JSON 1.0]: https://smithy.io/2.0/aws/protocols/aws-json-1_0-protocol.html
/// [AWS JSON 1.1]: https://smithy.io/2.0/aws/protocols/aws-json-1_1-protocol.html
#[cfg(feature = "http-02x")]
#[derive(Debug, Clone)]
pub struct AwsJsonRouter<S> {
    routes: TinyMap<&'static str, S, ROUTE_CUTOFF>,
}

#[cfg(feature = "http-02x")]
impl<S> AwsJsonRouter<S> {
    /// Applies a [`Layer`] uniformly to all routes.
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

    /// Applies type erasure to the inner route using [`Route::new`].
    #[cfg(feature = "http-02x")]
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

#[cfg(feature = "http-02x")]
impl<B, S> Router<B> for AwsJsonRouter<S>
where
    S: Clone,
{
    type Service = S;
    type Error = Error;

    #[cfg(feature = "http-02x")]
    fn match_route(&self, request: &http::Request<B>) -> Result<S, Self::Error> {
        // The URI must be root,
        if request.uri() != "/" {
            return Err(Error::NotRootUrl);
        }

        // Only `Method::POST` is allowed.
        #[cfg(feature = "http-02x")]
        if request.method() != http::Method::POST {
            return Err(Error::MethodNotAllowed);
        }

        // Find the `x-amz-target` header.
        let target = request.headers().get("x-amz-target").ok_or(Error::MissingHeader)?;
        let target = target.to_str().map_err(Error::InvalidHeader)?;

        // Lookup in the `TinyMap` for a route for the target.
        let route = self.routes.get(target).ok_or(Error::NotFound)?;
        Ok(route.clone())
    }
}

#[cfg(feature = "http-02x")]
impl<S> FromIterator<(&'static str, S)> for AwsJsonRouter<S> {
    #[inline]
    fn from_iter<T: IntoIterator<Item = (&'static str, S)>>(iter: T) -> Self {
        Self {
            routes: iter.into_iter().collect(),
        }
    }
}

#[cfg(all(test, feature = "http-02x"))]
mod tests {
    use super::*;
    use crate::{protocol::test_helpers::req, routing::Router};

    #[cfg(feature = "http-02x")]
    use http::{HeaderMap, HeaderValue, Method};
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn simple_routing() {
        let routes = vec![("Service.Operation")];
        let router: AwsJsonRouter<_> = routes.clone().into_iter().map(|operation| (operation, ())).collect();

        let mut headers = HeaderMap::new();
        headers.insert("x-amz-target", HeaderValue::from_static("Service.Operation"));

        // Valid request, should match.
        router
            .match_route(&req(&Method::POST, "/", Some(headers.clone())))
            .unwrap();

        // No headers, should return `MissingHeader`.
        let res = router.match_route(&req(&Method::POST, "/", None));
        assert_eq!(res.unwrap_err().to_string(), Error::MissingHeader.to_string());

        // Wrong HTTP method, should return `MethodNotAllowed`.
        let res = router.match_route(&req(&Method::GET, "/", Some(headers.clone())));
        assert_eq!(res.unwrap_err().to_string(), Error::MethodNotAllowed.to_string());

        // Wrong URI, should return `NotRootUrl`.
        let res = router.match_route(&req(&Method::POST, "/something", Some(headers)));
        assert_eq!(res.unwrap_err().to_string(), Error::NotRootUrl.to_string());
    }
}
