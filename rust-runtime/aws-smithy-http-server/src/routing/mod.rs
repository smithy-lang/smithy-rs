/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP routing that adheres to the [Smithy specification].
//!
//! [Smithy specification]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html

use self::request_spec::RequestSpec;
use self::tiny_map::TinyMap;
use crate::body::{boxed, Body, BoxBody, HttpBody};
use crate::error::BoxError;
use crate::protocols::Protocol;
use crate::runtime_error::{RuntimeError, RuntimeErrorKind};
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
mod lambda_handler;

#[doc(hidden)]
pub mod request_spec;

mod route;
mod tiny_map;

pub use self::lambda_handler::LambdaHandler;
pub use self::{future::RouterFuture, into_make_service::IntoMakeService, route::Route};

/// The router is a [`tower::Service`] that routes incoming requests to other `Service`s
/// based on the request's URI and HTTP method or on some specific header setting the target operation.
/// The former is adhering to the [Smithy specification], while the latter is adhering to
/// the [AwsJson specification].
///
/// The router is also [Protocol] aware and currently supports REST based protocols like [restJson1] or [restXml]
/// and RPC based protocols like [awsJson1.0] or [awsJson1.1].
/// It currently does not support Smithy's [endpoint trait].
///
/// You should not **instantiate** this router directly; it will be created for you from the
/// code generated from your Smithy model by `smithy-rs`.
///
/// [Smithy specification]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html
/// [AwsJson specification]: https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_0-protocol.html#protocol-behaviors
/// [Protocol]: https://awslabs.github.io/smithy/1.0/spec/aws/index.html#aws-protocols
/// [restJson1]: https://awslabs.github.io/smithy/1.0/spec/aws/aws-restjson1-protocol.html
/// [restXml]: https://awslabs.github.io/smithy/1.0/spec/aws/aws-restxml-protocol.html
/// [awsJson1.0]: https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_0-protocol.html
/// [awsJson1.1]: https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_1-protocol.html
/// [endpoint trait]: https://awslabs.github.io/smithy/1.0/spec/core/endpoint-traits.html#endpoint-trait
#[derive(Debug)]
pub struct Router<B = Body> {
    routes: Routes<B>,
}

// This constant determines when the `TinyMap` implementation switches from being a `Vec` to a
// `HashMap`. This is chosen to be 15 as a result of the discussion around
// https://github.com/awslabs/smithy-rs/pull/1429#issuecomment-1147516546
const ROUTE_CUTOFF: usize = 15;

/// Protocol-aware routes types.
///
/// RestJson1 and RestXml routes are stored in a `Vec` because there can be multiple matches on the
/// request URI and we thus need to iterate the whole list and use a ranking mechanism to choose.
///
/// AwsJson 1.0 and 1.1 routes can be stored in a `HashMap` since the requested operation can be
/// directly found in the `X-Amz-Target` HTTP header.
#[derive(Debug)]
enum Routes<B = Body> {
    RestXml(Vec<(Route<B>, RequestSpec)>),
    RestJson1(Vec<(Route<B>, RequestSpec)>),
    AwsJson10(TinyMap<String, Route<B>, ROUTE_CUTOFF>),
    AwsJson11(TinyMap<String, Route<B>, ROUTE_CUTOFF>),
}

impl<B> Clone for Router<B> {
    fn clone(&self) -> Self {
        match &self.routes {
            Routes::RestJson1(routes) => Router {
                routes: Routes::RestJson1(routes.clone()),
            },
            Routes::RestXml(routes) => Router {
                routes: Routes::RestXml(routes.clone()),
            },
            Routes::AwsJson10(routes) => Router {
                routes: Routes::AwsJson10(routes.clone()),
            },
            Routes::AwsJson11(routes) => Router {
                routes: Routes::AwsJson11(routes.clone()),
            },
        }
    }
}

impl<B> Router<B>
where
    B: Send + 'static,
{
    /// Return the correct, protocol-specific "Not Found" response for an unknown operation.
    fn unknown_operation(&self) -> RouterFuture<B> {
        let protocol = match &self.routes {
            Routes::RestJson1(_) => Protocol::RestJson1,
            Routes::RestXml(_) => Protocol::RestXml,
            Routes::AwsJson10(_) => Protocol::AwsJson10,
            Routes::AwsJson11(_) => Protocol::AwsJson11,
        };
        let error = RuntimeError {
            protocol,
            kind: RuntimeErrorKind::UnknownOperation,
        };
        RouterFuture::from_response(error.into_response())
    }

    /// Return the HTTP error response for non allowed method.
    fn method_not_allowed(&self) -> RouterFuture<B> {
        RouterFuture::from_response({
            let mut res = Response::new(crate::body::empty());
            *res.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
            res
        })
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
        match self.routes {
            Routes::RestJson1(routes) => {
                let routes = routes
                    .into_iter()
                    .map(|(route, request_spec)| (Layer::layer(&layer, route), request_spec))
                    .collect();
                Router {
                    routes: Routes::RestJson1(routes),
                }
            }
            Routes::RestXml(routes) => {
                let routes = routes
                    .into_iter()
                    .map(|(route, request_spec)| (Layer::layer(&layer, route), request_spec))
                    .collect();
                Router {
                    routes: Routes::RestXml(routes),
                }
            }
            Routes::AwsJson10(routes) => {
                let routes = routes
                    .into_iter()
                    .map(|(operation, route)| (operation, Layer::layer(&layer, route)))
                    .collect();
                Router {
                    routes: Routes::AwsJson10(routes),
                }
            }
            Routes::AwsJson11(routes) => {
                let routes = routes
                    .into_iter()
                    .map(|(operation, route)| (operation, Layer::layer(&layer, route)))
                    .collect();
                Router {
                    routes: Routes::AwsJson11(routes),
                }
            }
        }
    }

    /// Create a new RestJson1 `Router` from an iterator over pairs of [`RequestSpec`]s and services.
    ///
    /// If the iterator is empty the router will respond `404 Not Found` to all requests.
    #[doc(hidden)]
    pub fn new_rest_json_router<T>(routes: T) -> Self
    where
        T: IntoIterator<
            Item = (
                tower::util::BoxCloneService<Request<B>, Response<BoxBody>, Infallible>,
                RequestSpec,
            ),
        >,
    {
        let mut routes: Vec<(Route<B>, RequestSpec)> = routes
            .into_iter()
            .map(|(svc, request_spec)| (Route::from_box_clone_service(svc), request_spec))
            .collect();

        // Sort them once by specifity, with the more specific routes sorted before the less
        // specific ones, so that when routing a request we can simply iterate through the routes
        // and pick the first one that matches.
        routes.sort_by_key(|(_route, request_spec)| std::cmp::Reverse(request_spec.rank()));

        Self {
            routes: Routes::RestJson1(routes),
        }
    }

    /// Create a new RestXml `Router` from an iterator over pairs of [`RequestSpec`]s and services.
    ///
    /// If the iterator is empty the router will respond `404 Not Found` to all requests.
    #[doc(hidden)]
    pub fn new_rest_xml_router<T>(routes: T) -> Self
    where
        T: IntoIterator<
            Item = (
                tower::util::BoxCloneService<Request<B>, Response<BoxBody>, Infallible>,
                RequestSpec,
            ),
        >,
    {
        let mut routes: Vec<(Route<B>, RequestSpec)> = routes
            .into_iter()
            .map(|(svc, request_spec)| (Route::from_box_clone_service(svc), request_spec))
            .collect();

        // Sort them once by specifity, with the more specific routes sorted before the less
        // specific ones, so that when routing a request we can simply iterate through the routes
        // and pick the first one that matches.
        routes.sort_by_key(|(_route, request_spec)| std::cmp::Reverse(request_spec.rank()));

        Self {
            routes: Routes::RestXml(routes),
        }
    }

    /// Create a new AwsJson 1.0 `Router` from an iterator over pairs of operation names and services.
    ///
    /// If the iterator is empty the router will respond `404 Not Found` to all requests.
    #[doc(hidden)]
    pub fn new_aws_json_10_router<T>(routes: T) -> Self
    where
        T: IntoIterator<
            Item = (
                tower::util::BoxCloneService<Request<B>, Response<BoxBody>, Infallible>,
                String,
            ),
        >,
    {
        let routes = routes
            .into_iter()
            .map(|(svc, operation)| (operation, Route::from_box_clone_service(svc)))
            .collect();

        Self {
            routes: Routes::AwsJson10(routes),
        }
    }

    /// Create a new AwsJson 1.1 `Router` from a vector of pairs of operations and services.
    ///
    /// If the vector is empty the router will respond `404 Not Found` to all requests.
    #[doc(hidden)]
    pub fn new_aws_json_11_router<T>(routes: T) -> Self
    where
        T: IntoIterator<
            Item = (
                tower::util::BoxCloneService<Request<B>, Response<BoxBody>, Infallible>,
                String,
            ),
        >,
    {
        let routes = routes
            .into_iter()
            .map(|(svc, operation)| (operation, Route::from_box_clone_service(svc)))
            .collect();

        Self {
            routes: Routes::AwsJson11(routes),
        }
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
        match &self.routes {
            // REST routes.
            Routes::RestJson1(routes) | Routes::RestXml(routes) => {
                let mut method_not_allowed = false;

                // Loop through all the routes and validate if any of them matches. Routes are already ranked.
                for (route, request_spec) in routes {
                    match request_spec.matches(&req) {
                        request_spec::Match::Yes => {
                            return RouterFuture::from_oneshot(route.clone().oneshot(req));
                        }
                        request_spec::Match::MethodNotAllowed => method_not_allowed = true,
                        // Continue looping to see if another route matches.
                        request_spec::Match::No => continue,
                    }
                }

                if method_not_allowed {
                    // The HTTP method is not correct.
                    self.method_not_allowed()
                } else {
                    // In any other case return the `RuntimeError::UnknownOperation`.
                    self.unknown_operation()
                }
            }
            // AwsJson routes.
            Routes::AwsJson10(routes) | Routes::AwsJson11(routes) => {
                if req.uri() == "/" {
                    // Check the request method for POST.
                    if req.method() == http::Method::POST {
                        // Find the `x-amz-target` header.
                        if let Some(target) = req.headers().get("x-amz-target") {
                            if let Ok(target) = target.to_str() {
                                // Lookup in the `TinyMap` for a route for the target.
                                let route = routes.get(target);
                                if let Some(route) = route {
                                    return RouterFuture::from_oneshot(route.clone().oneshot(req));
                                }
                            }
                        }
                    } else {
                        // The HTTP method is not POST.
                        return self.method_not_allowed();
                    }
                }
                // In any other case return the `RuntimeError::UnknownOperation`.
                self.unknown_operation()
            }
        }
    }
}

#[cfg(test)]
mod rest_tests {
    use super::*;
    use crate::{
        body::{boxed, BoxBody},
        routing::request_spec::*,
    };
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

    // Returns a `Response`'s body as a `String`, without consuming the response.
    pub async fn get_body_as_string<B>(res: &mut Response<B>) -> String
    where
        B: http_body::Body + std::marker::Unpin,
        B::Error: std::fmt::Debug,
    {
        let body_mut = res.body_mut();
        let body_bytes = hyper::body::to_bytes(body_mut).await.unwrap();
        String::from(std::str::from_utf8(&body_bytes).unwrap())
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
        let router_json = Router::new_rest_json_router(request_specs.clone().into_iter().map(|(spec, svc_name)| {
            (
                tower::util::BoxCloneService::new(NamedEchoUriService(String::from(svc_name))),
                spec,
            )
        }));
        let router_xml = Router::new_rest_xml_router(request_specs.into_iter().map(|(spec, svc_name)| {
            (
                tower::util::BoxCloneService::new(NamedEchoUriService(String::from(svc_name))),
                spec,
            )
        }));

        for mut router in [router_json, router_xml] {
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
                let actual_body = get_body_as_string(&mut res).await;

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
    }

    #[tokio::test]
    async fn basic_pattern_conflict_avoidance() {
        let request_specs: Vec<(RequestSpec, &str)> = vec![
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

        let mut router = Router::new_rest_json_router(request_specs.into_iter().map(|(spec, svc_name)| {
            (
                tower::util::BoxCloneService::new(NamedEchoUriService(String::from(svc_name))),
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
            let actual_body = get_body_as_string(&mut res).await;

            assert_eq!(format!("{} :: {}", svc_name, uri), actual_body);
        }
    }
}

#[cfg(test)]
mod awsjson_tests {
    use super::rest_tests::{get_body_as_string, req};
    use super::*;
    use crate::body::boxed;
    use futures_util::Future;
    use http::{HeaderMap, HeaderValue, Method};
    use pretty_assertions::assert_eq;
    use std::pin::Pin;

    /// A service that returns its name and the request's URI in the response body.
    #[derive(Clone)]
    struct NamedEchoOperationService(String);

    impl<B> Service<Request<B>> for NamedEchoOperationService {
        type Response = Response<BoxBody>;
        type Error = Infallible;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        #[inline]
        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        #[inline]
        fn call(&mut self, req: Request<B>) -> Self::Future {
            let target = req
                .headers()
                .get("x-amz-target")
                .map(|x| x.to_str().unwrap())
                .unwrap_or("unknown");
            let body = boxed(Body::from(format!("{} :: {}", self.0, target)));
            let fut = async { Ok(Response::builder().status(&http::StatusCode::OK).body(body).unwrap()) };
            Box::pin(fut)
        }
    }

    #[tokio::test]
    async fn simple_routing() {
        let routes = vec![("Service.Operation", "A")];
        let router_json10 = Router::new_aws_json_10_router(routes.clone().into_iter().map(|(operation, svc_name)| {
            (
                tower::util::BoxCloneService::new(NamedEchoOperationService(String::from(svc_name))),
                operation.to_string(),
            )
        }));
        let router_json11 = Router::new_aws_json_11_router(routes.into_iter().map(|(operation, svc_name)| {
            (
                tower::util::BoxCloneService::new(NamedEchoOperationService(String::from(svc_name))),
                operation.to_string(),
            )
        }));

        for mut router in [router_json10, router_json11] {
            let mut headers = HeaderMap::new();
            headers.insert("x-amz-target", HeaderValue::from_static("Service.Operation"));

            // Valid request, should return a valid body.
            let mut res = router
                .call(req(&Method::POST, "/", Some(headers.clone())))
                .await
                .unwrap();
            let actual_body = get_body_as_string(&mut res).await;
            assert_eq!(format!("{} :: {}", "A", "Service.Operation"), actual_body);

            // No headers, should return NOT_FOUND.
            let res = router.call(req(&Method::POST, "/", None)).await.unwrap();
            assert_eq!(res.status(), StatusCode::NOT_FOUND);

            // Wrong HTTP method, should return METHOD_NOT_ALLOWED.
            let res = router
                .call(req(&Method::GET, "/", Some(headers.clone())))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);

            // Wrong URI, should return NOT_FOUND.
            let res = router
                .call(req(&Method::POST, "/something", Some(headers)))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::NOT_FOUND);
        }
    }
}
