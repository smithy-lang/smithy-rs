/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! HTTP routing that adheres to the [Smithy specification].
//!
//! [Smithy specification]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html

use self::future::RouterFuture;
use self::request_spec::RequestSpec;
use crate::body::{boxed, Body, BoxBody, HttpBody};
use crate::runtime_error::{RuntimeError, RuntimeErrorKind};
use crate::BoxError;
use axum_core::response::IntoResponse;
use http::{Request, Response, StatusCode};
use std::{
    convert::Infallible,
    task::{Context, Poll},
};
use tower::layer::Layer;
use tower::util::ServiceExt;
use tower::{Service, ServiceBuilder};
use tower_http::map_response_body::MapResponseBodyLayer;

mod future;
mod into_make_service;

#[doc(hidden)]
pub mod request_spec;

mod route;

pub use self::{into_make_service::IntoMakeService, route::Route};

/// The router is a [`tower::Service`] that routes incoming requests to other `Service`s
/// based on the request's URI and HTTP method, adhering to the [Smithy specification].
/// It currently does not support Smithy's [endpoint trait].
///
/// You should not **instantiate** this router directly; it will be created for you from the
/// code generated from your Smithy model by `smithy-rs`.
///
/// [Smithy specification]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html
/// [endpoint trait]: https://awslabs.github.io/smithy/1.0/spec/core/endpoint-traits.html#endpoint-trait
#[derive(Debug)]
pub struct Router<S, B = Body> {
    routes: Vec<(Route<B>, S)>,
}

impl<S, B> Clone for Router<S, B>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            routes: self.routes.clone(),
        }
    }
}

impl<S, B> Default for Router<S, B> {
    fn default() -> Self {
        Self {
            routes: Default::default(),
        }
    }
}

impl<S, B> Router<S, B>
where
    B: Send + 'static,
    S: RequestSpec,
{
    /// Create a new `Router` from a vector of pairs of request specs and services.
    ///
    /// If the vector is empty the router will respond `404 Not Found` to all requests.
    #[doc(hidden)]
    pub fn from_box_clone_service_iter<T>(routes: T) -> Self
    where
        T: IntoIterator<
            Item = (
                tower::util::BoxCloneService<Request<B>, Response<BoxBody>, Infallible>,
                S,
            ),
        >,
    {
        let mut routes: Vec<(Route<B>, S)> = routes
            .into_iter()
            .map(|(svc, request_spec)| (Route::from_box_clone_service(svc), request_spec))
            .collect();

        // Sort them once by specifity, with the more specific routes sorted before the less
        // specific ones, so that when routing a request we can simply iterate through the routes
        // and pick the first one that matches.
        routes.sort_by_key(|(_route, request_spec)| std::cmp::Reverse(request_spec.rank()));

        Self { routes }
    }

    /// Convert this router into a [`MakeService`], that is a [`Service`] whose
    /// response is another service.
    ///
    /// This is useful when running your application with hyper's
    /// [`Server`].
    ///
    /// [`Server`]: hyper::server::Server
    /// [`MakeService`]: tower::make::MakeService
    pub fn into_make_service(self) -> IntoMakeService<Self> {
        IntoMakeService::new(self)
    }

    /// Apply a [`tower::Layer`] to the router.
    ///
    /// All requests to the router will be processed by the layer's
    /// corresponding middleware.
    ///
    /// This can be used to add additional processing to all routes.
    pub fn layer<L, NewReqBody, NewResBody>(self, layer: L) -> Router<S, NewReqBody>
    where
        L: Layer<Route<B>>,
        L::Service:
            Service<Request<NewReqBody>, Response = Response<NewResBody>, Error = Infallible> + Clone + Send + 'static,
        <L::Service as Service<Request<NewReqBody>>>::Future: Send + 'static,
        NewResBody: HttpBody<Data = bytes::Bytes> + Send + 'static,
        NewResBody::Error: Into<BoxError>,
    {
        let layer = ServiceBuilder::new()
            .layer_fn(Route::new)
            .layer(MapResponseBodyLayer::new(boxed))
            .layer(layer);
        let routes = self
            .routes
            .into_iter()
            .map(|(route, request_spec)| (Layer::layer(&layer, route), request_spec))
            .collect();
        Router { routes }
    }
}

impl<S, B> Service<Request<B>> for Router<S, B>
where
    B: Send + 'static,
    S: RequestSpec,
{
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = RouterFuture<B>;

    #[inline]
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn call(&mut self, req: Request<B>) -> Self::Future {
        for (route, request_spec) in &self.routes {
            match request_spec.matches(&req) {
                request_spec::Match::Yes => {
                    return RouterFuture::from_oneshot(route.clone().oneshot(req));
                }
                request_spec::Match::MethodNotAllowed => {
                    let error = RuntimeError {
                        protocol: request_spec.protocol(),
                        kind: RuntimeErrorKind::UnknownOperation,
                    };
                    return RouterFuture::from_response(error.into_response());
                }
                // Continue looping to see if another route matches.
                request_spec::Match::No => continue,
            }
        }

        RouterFuture::from_response(
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(crate::body::empty())
                .unwrap(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::Protocol;
    use crate::{body::boxed, routing::request_spec::*};
    use futures_util::Future;
    use http::{HeaderMap, Method};
    use std::pin::Pin;

    /// Helper function to build a `Request`. Used in other test modules.
    pub fn req(method: &Method, uri: &str, headers: Option<HeaderMap>) -> Request<()> {
        let mut r = Request::builder().method(method).uri(uri).body(()).unwrap();
        if let Some(headers) = headers {
            *r.headers_mut() = headers
        }
        r
    }

    /// A REST service that returns its name and the request's URI in the response body.
    #[derive(Clone)]
    struct RestNamedEchoUriService(String);

    impl<B> Service<Request<B>> for RestNamedEchoUriService {
        type Response = Response<BoxBody>;
        type Error = Infallible;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        #[inline]
        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        #[inline]
        fn call(&mut self, req: Request<B>) -> Self::Future {
            let body = boxed(Body::from(format!("{} :: {}", self.0, req.uri().to_string())));
            let fut = async { Ok(Response::builder().status(&http::StatusCode::OK).body(body).unwrap()) };
            Box::pin(fut)
        }
    }

    // Returns a `Response`'s body as a `String`, without consuming the response.
    async fn get_body_as_str<B>(res: &mut Response<B>) -> String
    where
        B: http_body::Body + std::marker::Unpin,
        B::Error: std::fmt::Debug,
    {
        let body_mut = res.body_mut();
        let body_bytes = hyper::body::to_bytes(body_mut).await.unwrap();
        String::from(std::str::from_utf8(&body_bytes).unwrap())
    }

    // This test is a rewrite of `mux.spec.ts`.
    // https://github.com/awslabs/smithy-typescript/blob/fbf97a9bf4c1d8cf7f285ea7c24e1f0ef280142a/smithy-typescript-ssdk-libs/server-common/src/httpbinding/mux.spec.ts
    #[tokio::test]
    async fn rest_simple_routing() {
        let request_specs: Vec<(RestRequestSpec, &str)> = vec![
            (
                RestRequestSpec::from_parts(
                    Method::GET,
                    vec![
                        PathSegment::Literal(String::from("a")),
                        PathSegment::Label,
                        PathSegment::Label,
                    ],
                    Vec::new(),
                    Protocol::RestJson1,
                ),
                "A",
            ),
            (
                RestRequestSpec::from_parts(
                    Method::GET,
                    vec![
                        PathSegment::Literal(String::from("mg")),
                        PathSegment::Greedy,
                        PathSegment::Literal(String::from("z")),
                    ],
                    Vec::new(),
                    Protocol::RestJson1,
                ),
                "MiddleGreedy",
            ),
            (
                RestRequestSpec::from_parts(
                    Method::DELETE,
                    Vec::new(),
                    vec![
                        QuerySegment::KeyValue(String::from("foo"), String::from("bar")),
                        QuerySegment::Key(String::from("baz")),
                    ],
                    Protocol::RestJson1,
                ),
                "Delete",
            ),
            (
                RestRequestSpec::from_parts(
                    Method::POST,
                    vec![PathSegment::Literal(String::from("query_key_only"))],
                    vec![QuerySegment::Key(String::from("foo"))],
                    Protocol::RestJson1,
                ),
                "QueryKeyOnly",
            ),
        ];

        let mut router = Router::from_box_clone_service_iter(request_specs.into_iter().map(|(spec, svc_name)| {
            (
                tower::util::BoxCloneService::new(RestNamedEchoUriService(String::from(svc_name))),
                spec,
            )
        }));

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
            let mut res = router.call(req(method, uri, None)).await.unwrap();
            let actual_body = get_body_as_str(&mut res).await;

            assert_eq!(format!("{} :: {}", svc_name, uri), actual_body);
        }

        for (_, _, uri) in hits {
            let res = router.call(req(&Method::PATCH, uri, None)).await.unwrap();
            assert_eq!(StatusCode::METHOD_NOT_ALLOWED, res.status());
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
            let res = router.call(req(&method, miss, None)).await.unwrap();
            assert_eq!(StatusCode::NOT_FOUND, res.status());
        }
    }

    #[tokio::test]
    async fn rest_basic_pattern_conflict_avoidance() {
        let request_specs: Vec<(RestRequestSpec, &str)> = vec![
            (
                RestRequestSpec::from_parts(
                    Method::GET,
                    vec![PathSegment::Literal(String::from("a")), PathSegment::Label],
                    Vec::new(),
                    Protocol::RestJson1,
                ),
                "A1",
            ),
            (
                RestRequestSpec::from_parts(
                    Method::GET,
                    vec![
                        PathSegment::Literal(String::from("a")),
                        PathSegment::Label,
                        PathSegment::Literal(String::from("a")),
                    ],
                    Vec::new(),
                    Protocol::RestJson1,
                ),
                "A2",
            ),
            (
                RestRequestSpec::from_parts(
                    Method::GET,
                    vec![PathSegment::Literal(String::from("b")), PathSegment::Greedy],
                    Vec::new(),
                    Protocol::RestJson1,
                ),
                "B1",
            ),
            (
                RestRequestSpec::from_parts(
                    Method::GET,
                    vec![PathSegment::Literal(String::from("b")), PathSegment::Greedy],
                    vec![QuerySegment::Key(String::from("q"))],
                    Protocol::RestJson1,
                ),
                "B2",
            ),
        ];

        let mut router = Router::from_box_clone_service_iter(request_specs.into_iter().map(|(spec, svc_name)| {
            (
                tower::util::BoxCloneService::new(RestNamedEchoUriService(String::from(svc_name))),
                spec,
            )
        }));

        let hits = vec![
            ("A1", Method::GET, "/a/foo"),
            ("A2", Method::GET, "/a/foo/a"),
            ("B1", Method::GET, "/b/foo/bar/baz"),
            ("B2", Method::GET, "/b/foo?q=baz"),
        ];
        for (svc_name, method, uri) in &hits {
            let mut res = router.call(req(method, uri, None)).await.unwrap();
            let actual_body = get_body_as_str(&mut res).await;

            assert_eq!(format!("{} :: {}", svc_name, uri), actual_body);
        }
    }
}
