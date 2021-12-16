/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! HTTP routing that adheres to the [Smithy specification].
//!
//! [Smithy specification]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html

use self::{future::RouterFuture, request_spec::RequestSpec};
use crate::body::{boxed, Body, BoxBody, HttpBody};
use crate::BoxError;
use http::{Request, Response, StatusCode};
use std::{
    convert::Infallible,
    task::{Context, Poll},
};
use tower::layer::Layer;
use tower::util::ServiceExt;
use tower::{Service, ServiceBuilder};
use tower_http::map_response_body::MapResponseBodyLayer;

pub mod future;
mod into_make_service;
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
pub struct Router<B = Body> {
    routes: Vec<(Route<B>, RequestSpec)>,
}

impl<B> Clone for Router<B> {
    fn clone(&self) -> Self {
        Self {
            routes: self.routes.clone(),
        }
    }
}

impl<B> Default for Router<B>
where
    B: Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<B> Router<B>
where
    B: Send + 'static,
{
    /// Create a new `Router`.
    ///
    /// Unless you add additional routes this will respond to `404 Not Found` to
    /// all requests.
    #[doc(hidden)]
    pub fn new() -> Self {
        Self {
            routes: Default::default(),
        }
    }

    /// Add a route to the router.
    #[doc(hidden)]
    pub fn route<T>(mut self, request_spec: RequestSpec, svc: T) -> Self
    where
        T: Service<Request<B>, Response = Response<BoxBody>, Error = Infallible> + Clone + Send + 'static,
        T::Future: Send + 'static,
    {
        self.routes.push((Route::new(svc), request_spec));
        self
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
    pub fn layer<L, NewReqBody, NewResBody>(self, layer: L) -> Router<NewReqBody>
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

impl<B> Service<Request<B>> for Router<B>
where
    B: Send + 'static,
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
        let mut method_not_allowed = false;

        for (route, request_spec) in &self.routes {
            match request_spec.matches(&req) {
                request_spec::Match::Yes => {
                    return RouterFuture::from_oneshot(route.clone().oneshot(req));
                }
                request_spec::Match::MethodNotAllowed => method_not_allowed = true,
                // Continue looping to see if another route matches.
                request_spec::Match::No => continue,
            }
        }

        let status_code = if method_not_allowed {
            StatusCode::METHOD_NOT_ALLOWED
        } else {
            StatusCode::NOT_FOUND
        };
        RouterFuture::from_response(
            Response::builder()
                .status(status_code)
                .body(crate::body::empty())
                .unwrap(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{body::boxed, routing::request_spec::*};
    use futures_util::Future;
    use http::Method;
    use std::pin::Pin;

    /// Helper function to build a `Request`. Used in other test modules.
    pub fn req(method: &Method, uri: &str) -> Request<()> {
        Request::builder().method(method).uri(uri).body(()).unwrap()
    }

    /// A service that returns its name and the request's URI in the response body.
    #[derive(Clone)]
    struct NamedEchoUriService(String);

    impl<B> Service<Request<B>> for NamedEchoUriService {
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
    async fn simple_routing() {
        let request_specs: Vec<(RequestSpec, &str)> = vec![
            (
                RequestSpec::from_parts(
                    Method::GET,
                    vec![
                        PathSegment::Literal(String::from("a")),
                        PathSegment::Label,
                        PathSegment::Label,
                    ],
                    vec![],
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
                    vec![],
                ),
                "MiddleGreedy",
            ),
            (
                RequestSpec::from_parts(
                    Method::DELETE,
                    vec![],
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

        let mut router = Router::new();
        for (spec, svc_name) in request_specs {
            let svc = NamedEchoUriService(String::from(svc_name));
            router = router.route(spec, svc.clone());
        }

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
            let mut res = router.call(req(method, uri)).await.unwrap();
            let actual_body = get_body_as_str(&mut res).await;

            assert_eq!(format!("{} :: {}", svc_name, uri), actual_body);
        }

        for (_, _, uri) in hits {
            let res = router.call(req(&Method::PATCH, uri)).await.unwrap();
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
            let res = router.call(req(&method, miss)).await.unwrap();
            assert_eq!(StatusCode::NOT_FOUND, res.status());
        }
    }
}
