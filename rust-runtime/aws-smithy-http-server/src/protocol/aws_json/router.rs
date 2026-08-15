/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;

use tower::Layer;
use tower::Service;

use crate::body::BoxBody;
use crate::routing::tiny_map::TinyMap;
use crate::routing::PathPrefix;
use crate::routing::Route;
use crate::routing::Router;

use http::header::ToStrError;
use thiserror::Error;

/// An AWS JSON routing error.
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
    InvalidHeader(ToStrError),
    /// Operation not found.
    #[error("operation not found")]
    NotFound,
}

// This constant determines when the `TinyMap` implementation switches from being a `Vec` to a
// `HashMap`. This is chosen to be 15 as a result of the discussion around
// https://github.com/smithy-lang/smithy-rs/pull/1429#issuecomment-1147516546
pub(crate) const ROUTE_CUTOFF: usize = 15;

/// Routing configuration for an AWS JSON operation.
#[derive(Debug, Clone)]
pub struct AwsJsonRequestSpec {
    route_key: &'static str,
    path_prefix: Option<PathPrefix>,
}

impl AwsJsonRequestSpec {
    /// Creates routing configuration for an operation target.
    pub const fn new(route_key: &'static str) -> Self {
        Self {
            route_key,
            path_prefix: None,
        }
    }

    /// Configures operation-local path prefixes for this request specification.
    pub fn with_path_prefix(mut self, path_prefix: PathPrefix) -> Self {
        self.path_prefix = Some(path_prefix);
        self
    }
}

#[derive(Debug, Clone)]
struct RouteEntry<S> {
    inner: S,
    path_prefix: Option<PathPrefix>,
}

/// A [`Router`] supporting [AWS JSON 1.0] and [AWS JSON 1.1] protocols.
///
/// [AWS JSON 1.0]: https://smithy.io/2.0/aws/protocols/aws-json-1_0-protocol.html
/// [AWS JSON 1.1]: https://smithy.io/2.0/aws/protocols/aws-json-1_1-protocol.html
#[derive(Debug, Clone)]
pub struct AwsJsonRouter<S> {
    routes: TinyMap<&'static str, RouteEntry<S>, ROUTE_CUTOFF>,
}

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
                .map(|(key, route)| {
                    (
                        key,
                        RouteEntry {
                            inner: layer.layer(route.inner),
                            path_prefix: route.path_prefix,
                        },
                    )
                })
                .collect(),
        }
    }

    /// Applies type erasure to the inner route using [`Route::new`].
    pub fn boxed<B>(self) -> AwsJsonRouter<Route<B>>
    where
        S: Service<http::Request<B>, Response = http::Response<BoxBody>, Error = Infallible>,
        S: Send + Clone + 'static,
        S::Future: Send + 'static,
    {
        AwsJsonRouter {
            routes: self
                .routes
                .into_iter()
                .map(|(key, route)| {
                    (
                        key,
                        RouteEntry {
                            inner: Route::new(route.inner),
                            path_prefix: route.path_prefix,
                        },
                    )
                })
                .collect(),
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
        // Only `Method::POST` is allowed.
        if request.method() != http::Method::POST {
            return Err(Error::MethodNotAllowed);
        }

        // Find the `x-amz-target` header.
        let target = request.headers().get("x-amz-target").ok_or(Error::MissingHeader)?;
        let target = target.to_str().map_err(Error::InvalidHeader)?;

        // Lookup in the `TinyMap` for a route for the target.
        let route = self.routes.get(target).ok_or(Error::NotFound)?;

        match &route.path_prefix {
            // Preserve the existing root-only behavior for services without modeled prefixes.
            None if request.uri() != "/" => return Err(Error::NotRootUrl),
            None => {}
            Some(path_prefix) => {
                let route_path = path_prefix.match_uri_path(request.uri()).ok_or(Error::NotRootUrl)?;
                if route_path != "/" || request.uri().query().is_some() {
                    path_prefix.log_rejection(request.uri());
                    return Err(Error::NotRootUrl);
                }
            }
        }

        Ok(route.inner.clone())
    }
}

impl<S> FromIterator<(AwsJsonRequestSpec, S)> for AwsJsonRouter<S> {
    #[inline]
    fn from_iter<T: IntoIterator<Item = (AwsJsonRequestSpec, S)>>(iter: T) -> Self {
        Self {
            routes: iter
                .into_iter()
                .map(|(spec, inner)| {
                    (
                        spec.route_key,
                        RouteEntry {
                            inner,
                            path_prefix: spec.path_prefix,
                        },
                    )
                })
                .collect(),
        }
    }
}

impl<S> FromIterator<(&'static str, S)> for AwsJsonRouter<S> {
    #[inline]
    fn from_iter<T: IntoIterator<Item = (&'static str, S)>>(iter: T) -> Self {
        Self {
            routes: iter
                .into_iter()
                .map(|(route_key, inner)| {
                    (
                        route_key,
                        RouteEntry {
                            inner,
                            path_prefix: None,
                        },
                    )
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{protocol::test_helpers::req, routing::Router};

    use http::{HeaderMap, HeaderValue, Method};
    use pretty_assertions::assert_eq;

    fn headers_with_amz_target(target: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-amz-target", HeaderValue::from_static(target));
        headers
    }

    #[tokio::test]
    async fn simple_routing() {
        let router: AwsJsonRouter<_> = [("Service.Operation", ())].into_iter().collect();
        let headers = headers_with_amz_target("Service.Operation");

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

    #[test]
    fn operation_local_path_prefixes() {
        let router: AwsJsonRouter<_> = [
            (
                AwsJsonRequestSpec::new("Service.First").with_path_prefix(PathPrefix::new(&["v1", "internal"], false)),
                "first",
            ),
            (
                AwsJsonRequestSpec::new("Service.Second").with_path_prefix(PathPrefix::new(&["write"], true)),
                "second",
            ),
            (AwsJsonRequestSpec::new("Service.Original"), "original"),
        ]
        .into_iter()
        .collect();

        for (target, uri, expected) in [
            ("Service.First", "/v1", "first"),
            ("Service.First", "/internal/", "first"),
            ("Service.Second", "/write", "second"),
            ("Service.Second", "/", "second"),
            ("Service.Original", "/", "original"),
        ] {
            let request = req(&Method::POST, uri, Some(headers_with_amz_target(target)));
            assert_eq!(router.match_route(&request).unwrap(), expected);
        }

        for (target, uri) in [
            ("Service.First", "/"),
            ("Service.First", "/write"),
            ("Service.Second", "/v1"),
            ("Service.Second", "/unknown"),
            ("Service.Second", "/write/extra"),
            ("Service.Second", "/write?query=value"),
            ("Service.Original", "/arbitrary"),
        ] {
            let request = req(&Method::POST, uri, Some(headers_with_amz_target(target)));
            assert!(matches!(router.match_route(&request), Err(Error::NotRootUrl)));
        }
    }
}
