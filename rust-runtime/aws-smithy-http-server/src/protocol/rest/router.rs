/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;

use crate::body::BoxBody;
use crate::routing::request_spec::Match;
use crate::routing::request_spec::RequestSpec;
use crate::routing::Route;
use crate::routing::Router;
use tower::Layer;
use tower::Service;

use thiserror::Error;

/// An AWS REST routing error.
#[derive(Debug, Error, PartialEq)]
pub enum Error {
    /// Operation not found.
    #[error("operation not found")]
    NotFound,
    /// Method was not allowed.
    #[error("method was not allowed")]
    MethodNotAllowed,
}

/// A [`Router`] supporting [AWS restJson1] and [AWS restXml] protocols.
///
/// [AWS restJson1]: https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restjson1-protocol.html
/// [AWS restXml]: https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restxml-protocol.html
#[derive(Debug, Clone)]
pub struct RestRouter<S> {
    routes: Vec<(RequestSpec, S)>,
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
                .map(|(request_spec, route)| (request_spec, layer.layer(route)))
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
            routes: self.routes.into_iter().map(|(spec, s)| (spec, Route::new(s))).collect(),
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
        println!("[TRACE 9] File: aws-smithy-http-server/src/protocol/rest/router.rs");
        println!("[TRACE 9] Type: RestRouter<S>");
        println!("[TRACE 9] Function: Router::match_route()");
        println!("[TRACE 9] Searching {} routes for match...", self.routes.len());

        let mut method_allowed = true;

        for (idx, (request_spec, route)) in self.routes.iter().enumerate() {
            let match_result = request_spec.matches(request);
            println!("[TRACE 10] Route[{}]: Checking RequestSpec - Result: {:?}", idx, match_result);

            match match_result {
                // Match found.
                Match::Yes => {
                    println!("[TRACE 11] File: aws-smithy-http-server/src/protocol/rest/router.rs");
                    println!("[TRACE 11] MATCH FOUND at route index {}!", idx);
                    return Ok(route.clone());
                }
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

impl<S> FromIterator<(RequestSpec, S)> for RestRouter<S> {
    #[inline]
    fn from_iter<T: IntoIterator<Item = (RequestSpec, S)>>(iter: T) -> Self {
        let mut routes: Vec<(RequestSpec, S)> = iter.into_iter().collect();

        // Sort them once by specificity, with the more specific routes sorted before the less
        // specific ones, so that when routing a request we can simply iterate through the routes
        // and pick the first one that matches.
        routes.sort_by_key(|(request_spec, _route)| std::cmp::Reverse(request_spec.rank()));

        Self { routes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{protocol::test_helpers::req, routing::request_spec::*};

    use http::Method;

    // This test is a rewrite of `mux.spec.ts`.
    // https://github.com/awslabs/smithy-typescript/blob/fbf97a9bf4c1d8cf7f285ea7c24e1f0ef280142a/smithy-typescript-ssdk-libs/server-common/src/httpbinding/mux.spec.ts
    #[test]
    fn simple_routing() {
        let request_specs: Vec<(RequestSpec, &'static str)> = vec![
            (
                RequestSpec::from_parts(
                    Method::GET,
                    vec![
                        PathSegment::Literal(String::from("a")),
                        PathSegment::Label,
                        PathSegment::Label,
                    ],
                    Vec::new(),
                ),
                "A",
            ),
            (
                RequestSpec::from_parts(
                    Method::GET,
                    vec![
                        PathSegment::Literal(String::from("mg")),
                        PathSegment::Greedy,
                        PathSegment::Literal(String::from("z")),
                    ],
                    Vec::new(),
                ),
                "MiddleGreedy",
            ),
            (
                RequestSpec::from_parts(
                    Method::DELETE,
                    Vec::new(),
                    vec![
                        QuerySegment::KeyValue(String::from("foo"), String::from("bar")),
                        QuerySegment::Key(String::from("baz")),
                    ],
                ),
                "Delete",
            ),
            (
                RequestSpec::from_parts(
                    Method::POST,
                    vec![PathSegment::Literal(String::from("query_key_only"))],
                    vec![QuerySegment::Key(String::from("foo"))],
                ),
                "QueryKeyOnly",
            ),
        ];

        // Test both RestJson1 and RestXml routers.
        let router: RestRouter<_> = request_specs.into_iter().collect();

        let hits = vec![
            ("A", Method::GET, "/a/b/c"),
            ("MiddleGreedy", Method::GET, "/mg/a/z"),
            ("MiddleGreedy", Method::GET, "/mg/a/b/c/d/z?abc=def"),
            ("Delete", Method::DELETE, "/?foo=bar&baz=quux"),
            ("Delete", Method::DELETE, "/?foo=bar&baz"),
            ("Delete", Method::DELETE, "/?foo=bar&baz=&"),
            ("Delete", Method::DELETE, "/?foo=bar&baz=quux&baz=grault"),
            ("QueryKeyOnly", Method::POST, "/query_key_only?foo=bar"),
            ("QueryKeyOnly", Method::POST, "/query_key_only?foo"),
            ("QueryKeyOnly", Method::POST, "/query_key_only?foo="),
            ("QueryKeyOnly", Method::POST, "/query_key_only?foo=&"),
        ];
        for (svc_name, method, uri) in &hits {
            assert_eq!(router.match_route(&req(method, uri, None)).unwrap(), *svc_name);
        }

        for (_, _, uri) in hits {
            let res = router.match_route(&req(&Method::PATCH, uri, None));
            assert_eq!(res.unwrap_err(), Error::MethodNotAllowed);
        }

        let misses = vec![
            (Method::GET, "/a"),
            (Method::GET, "/a/b"),
            (Method::GET, "/mg"),
            (Method::GET, "/mg/q"),
            (Method::GET, "/mg/z"),
            (Method::GET, "/mg/a/b/z/c"),
            (Method::DELETE, "/?foo=bar"),
            (Method::DELETE, "/?foo=bar"),
            (Method::DELETE, "/?baz=quux"),
            (Method::POST, "/query_key_only?baz=quux"),
            (Method::GET, "/"),
            (Method::POST, "/"),
        ];
        for (method, miss) in misses {
            let res = router.match_route(&req(&method, miss, None));
            assert_eq!(res.unwrap_err(), Error::NotFound);
        }
    }

    #[tokio::test]
    async fn basic_pattern_conflict_avoidance() {
        let request_specs: Vec<(RequestSpec, &'static str)> = vec![
            (
                RequestSpec::from_parts(
                    Method::GET,
                    vec![PathSegment::Literal(String::from("a")), PathSegment::Label],
                    Vec::new(),
                ),
                "A1",
            ),
            (
                RequestSpec::from_parts(
                    Method::GET,
                    vec![
                        PathSegment::Literal(String::from("a")),
                        PathSegment::Label,
                        PathSegment::Literal(String::from("a")),
                    ],
                    Vec::new(),
                ),
                "A2",
            ),
            (
                RequestSpec::from_parts(
                    Method::GET,
                    vec![PathSegment::Literal(String::from("b")), PathSegment::Greedy],
                    Vec::new(),
                ),
                "B1",
            ),
            (
                RequestSpec::from_parts(
                    Method::GET,
                    vec![PathSegment::Literal(String::from("b")), PathSegment::Greedy],
                    vec![QuerySegment::Key(String::from("q"))],
                ),
                "B2",
            ),
        ];

        let router: RestRouter<_> = request_specs.into_iter().collect();

        let hits = vec![
            ("A1", Method::GET, "/a/foo"),
            ("A2", Method::GET, "/a/foo/a"),
            ("B1", Method::GET, "/b/foo/bar/baz"),
            ("B2", Method::GET, "/b/foo?q=baz"),
        ];
        for (svc_name, method, uri) in hits {
            assert_eq!(router.match_route(&req(&method, uri, None)).unwrap(), svc_name);
        }
    }
}
