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
use http::header::CONTENT_TYPE;
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
    routes: Vec<(RestRouteSpec, S)>,
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
                .map(|(route_spec, route)| (route_spec, layer.layer(route)))
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

    /// Claims a REST route for multi-protocol routing.
    pub fn claim_route<B>(&self, request: &http::Request<B>) -> RestRouteClaim<S>
    where
        S: Clone,
    {
        let mut best_rejection = None;

        for (route_spec, route) in &self.routes {
            let route_rank = route_spec.request_spec.rank();
            match route_spec.request_spec.matches(request) {
                Match::Yes => {
                    if route_spec.request_content_type.claims(request) {
                        return RestRouteClaim::RouteMatched {
                            route: route.clone(),
                            route_rank,
                        };
                    }
                    if best_rejection.is_none() {
                        best_rejection = Some(RestRouteClaim::RejectedNonExclusive {
                            route_rank,
                            reason: RestClaimRejection::UnsupportedMediaType,
                        });
                    }
                }
                Match::MethodNotAllowed => {
                    if best_rejection.is_none() {
                        best_rejection = Some(RestRouteClaim::RejectedNonExclusive {
                            route_rank,
                            reason: RestClaimRejection::MethodNotAllowed,
                        });
                    }
                }
                Match::No => {}
            }
        }

        best_rejection.unwrap_or(RestRouteClaim::NoClaim)
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

        for (request_spec, route) in &self.routes {
            match request_spec.request_spec.matches(request) {
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

impl<S> FromIterator<(RestRouteSpec, S)> for RestRouter<S> {
    #[inline]
    fn from_iter<T: IntoIterator<Item = (RestRouteSpec, S)>>(iter: T) -> Self {
        let mut routes: Vec<(RestRouteSpec, S)> = iter.into_iter().collect();

        // Sort them once by specificity, with the more specific routes sorted before the less
        // specific ones, so that when routing a request we can simply iterate through the routes
        // and pick the first one that matches.
        routes.sort_by_key(|(route_spec, _route)| std::cmp::Reverse(route_spec.request_spec.rank()));

        Self { routes }
    }
}

impl<S> FromIterator<(RequestSpec, S)> for RestRouter<S> {
    #[inline]
    fn from_iter<T: IntoIterator<Item = (RequestSpec, S)>>(iter: T) -> Self {
        iter.into_iter()
            .map(|(request_spec, route)| {
                (
                    RestRouteSpec::new(request_spec, RequestContentType::AnyValidContentType { default: "" }),
                    route,
                )
            })
            .collect::<RestRouter<S>>()
    }
}

/// REST-specific route metadata used during multi-protocol route claiming.
#[derive(Debug, Clone)]
pub struct RestRouteSpec {
    request_spec: RequestSpec,
    request_content_type: RequestContentType,
}

impl RestRouteSpec {
    pub fn new(request_spec: RequestSpec, request_content_type: RequestContentType) -> Self {
        Self {
            request_spec,
            request_content_type,
        }
    }
}

/// Request `Content-Type` rule for REST protocol route claiming.
#[derive(Debug, Clone)]
pub enum RequestContentType {
    Expected(&'static str),
    AnyValidContentType { default: &'static str },
}

impl RequestContentType {
    fn claims<B>(&self, request: &http::Request<B>) -> bool {
        let Some(actual) = request.headers().get(CONTENT_TYPE) else {
            return false;
        };
        let Ok(actual) = actual.to_str() else {
            return false;
        };
        let Ok(actual) = actual.parse::<mime::Mime>() else {
            return false;
        };

        match self {
            Self::Expected(expected) => {
                let expected = expected
                    .parse::<mime::Mime>()
                    .expect("BUG: expected REST request content type generated by codegen must be valid MIME");
                expected == actual.essence_str()
            }
            Self::AnyValidContentType { default: _ } => true,
        }
    }
}

/// Result of REST route claiming.
#[derive(Debug, Clone)]
pub enum RestRouteClaim<S> {
    NoClaim,
    RouteMatched {
        route: S,
        route_rank: usize,
    },
    RejectedNonExclusive {
        route_rank: usize,
        reason: RestClaimRejection,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestClaimRejection {
    MethodNotAllowed,
    UnsupportedMediaType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestProtocolRejection {
    pub route_rank: usize,
    pub reason: RestClaimRejection,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{protocol::test_helpers::req, routing::request_spec::*};

    use http::Method;

    fn rest_route_spec(
        method: Method,
        path_segments: Vec<PathSegment>,
        query_segments: Vec<QuerySegment>,
        request_content_type: RequestContentType,
    ) -> RestRouteSpec {
        RestRouteSpec::new(
            RequestSpec::from_parts(method, path_segments, query_segments),
            request_content_type,
        )
    }

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

    #[test]
    fn claim_route_requires_expected_content_type() {
        let router: RestRouter<_> = vec![(
            rest_route_spec(
                Method::POST,
                vec![PathSegment::Literal(String::from("items"))],
                Vec::new(),
                RequestContentType::Expected("application/json"),
            ),
            "handler",
        )]
        .into_iter()
        .collect();

        let request = http::Request::builder()
            .method(Method::POST)
            .uri("/items")
            .header(http::header::CONTENT_TYPE, "application/json; charset=utf-8")
            .body(())
            .unwrap();
        assert!(matches!(
            router.claim_route(&request),
            RestRouteClaim::RouteMatched {
                route: "handler",
                route_rank: 1
            }
        ));

        let request = http::Request::builder()
            .method(Method::POST)
            .uri("/items")
            .header(http::header::CONTENT_TYPE, "application/xml")
            .body(())
            .unwrap();
        assert!(matches!(
            router.claim_route(&request),
            RestRouteClaim::RejectedNonExclusive {
                route_rank: 1,
                reason: RestClaimRejection::UnsupportedMediaType
            }
        ));

        let request = http::Request::builder()
            .method(Method::POST)
            .uri("/items")
            .body(())
            .unwrap();
        assert!(matches!(
            router.claim_route(&request),
            RestRouteClaim::RejectedNonExclusive {
                route_rank: 1,
                reason: RestClaimRejection::UnsupportedMediaType
            }
        ));
    }

    #[test]
    fn claim_route_allows_any_valid_content_type_when_modeled() {
        let router: RestRouter<_> = vec![(
            rest_route_spec(
                Method::POST,
                vec![PathSegment::Literal(String::from("items"))],
                Vec::new(),
                RequestContentType::AnyValidContentType {
                    default: "application/json",
                },
            ),
            "handler",
        )]
        .into_iter()
        .collect();

        let request = http::Request::builder()
            .method(Method::POST)
            .uri("/items")
            .header(http::header::CONTENT_TYPE, "text/plain")
            .body(())
            .unwrap();
        assert!(matches!(
            router.claim_route(&request),
            RestRouteClaim::RouteMatched {
                route: "handler",
                route_rank: 1
            }
        ));

        let request = http::Request::builder()
            .method(Method::POST)
            .uri("/items")
            .header(http::header::CONTENT_TYPE, "not a valid mime")
            .body(())
            .unwrap();
        assert!(matches!(
            router.claim_route(&request),
            RestRouteClaim::RejectedNonExclusive {
                reason: RestClaimRejection::UnsupportedMediaType,
                ..
            }
        ));
    }

    #[test]
    fn claim_route_wrong_method_is_non_exclusive_rejection() {
        let router: RestRouter<_> = vec![(
            rest_route_spec(
                Method::POST,
                vec![PathSegment::Literal(String::from("items"))],
                Vec::new(),
                RequestContentType::Expected("application/json"),
            ),
            "handler",
        )]
        .into_iter()
        .collect();

        let request = http::Request::builder()
            .method(Method::GET)
            .uri("/items")
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(())
            .unwrap();
        assert!(matches!(
            router.claim_route(&request),
            RestRouteClaim::RejectedNonExclusive {
                route_rank: 1,
                reason: RestClaimRejection::MethodNotAllowed
            }
        ));
    }
}
