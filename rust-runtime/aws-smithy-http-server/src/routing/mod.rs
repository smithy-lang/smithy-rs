/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP routing that adheres to the [Smithy specification].
//!
//! [Smithy specification]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html

mod into_make_service;
mod into_make_service_with_connect_info;
#[cfg(feature = "aws-lambda")]
#[cfg_attr(docsrs, doc(cfg(feature = "aws-lambda")))]
mod lambda_handler;

#[doc(hidden)]
pub mod request_spec;

mod route;

pub(crate) mod tiny_map;

#[cfg(feature = "aws-lambda")]
#[cfg_attr(docsrs, doc(cfg(feature = "aws-lambda")))]
pub use self::lambda_handler::LambdaHandler;

#[allow(deprecated)]
pub use self::{
    into_make_service::IntoMakeService,
    into_make_service_with_connect_info::{Connected, IntoMakeServiceWithConnectInfo},
    route::Route,
};

#[cfg(test)]
#[allow(deprecated)]
mod rest_tests {
    use super::*;
    use crate::{
        body::{boxed, BoxBody},
        routing::request_spec::*,
    };
    use futures_util::Future;
    use http::{HeaderMap, Method, StatusCode};
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
            let body = boxed(Body::from(format!("{} :: {}", self.0, req.uri())));
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

#[allow(deprecated)]
#[cfg(test)]
mod awsjson_tests {
    use super::rest_tests::{get_body_as_string, req};
    use super::*;
    use crate::body::boxed;
    use futures_util::Future;
    use http::{HeaderMap, HeaderValue, Method, StatusCode};
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
