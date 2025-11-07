/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    fmt::Debug,
    task::{Context, Poll},
};
use tower::Service;

use crate::http;
use lambda_http::{Request, RequestExt};

use crate::body::{self, BoxBodySync};

type ServiceRequest = http::Request<BoxBodySync>;

/// A [`Service`] that takes a `lambda_http::Request` and converts
/// it to `http::Request<BoxBody>`.
///
/// **This version is only guaranteed to be compatible with
/// [`lambda_http`](https://docs.rs/lambda_http) ^0.17.** Please ensure that your service crate's
/// `Cargo.toml` depends on a compatible version.
///
/// [`Service`]: tower::Service
#[derive(Debug, Clone)]
pub struct LambdaHandler<S> {
    service: S,
}

impl<S> LambdaHandler<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }
}

impl<S> Service<Request> for LambdaHandler<S>
where
    S: Service<ServiceRequest>,
{
    type Error = S::Error;
    type Response = S::Response;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&mut self, event: Request) -> Self::Future {
        self.service.call(convert_event(event))
    }
}

/// Converts a `lambda_http::Request` into a `http::Request<BoxBodySync>`
/// Issue: <https://github.com/smithy-lang/smithy-rs/issues/1125>
///
/// While converting the event the [API Gateway Stage] portion of the URI
/// is removed from the uri that gets returned as a new `http::Request`.
///
/// [API Gateway Stage]: https://docs.aws.amazon.com/apigateway/latest/developerguide/http-api-stages.html
fn convert_event(request: Request) -> ServiceRequest {
    let raw_path: &str = request.extensions().raw_http_path();
    let path: &str = request.uri().path();

    let (parts, body) = if !raw_path.is_empty() && raw_path != path {
        let mut path = raw_path.to_owned(); // Clone only when we need to strip out the stage.
        let (mut parts, body) = request.into_parts();

        let uri_parts: http::uri::Parts = parts.uri.into();
        let path_and_query = uri_parts
            .path_and_query
            .expect("request URI does not have `PathAndQuery`");

        if let Some(query) = path_and_query.query() {
            path.push('?');
            path.push_str(query);
        }

        parts.uri = http::Uri::builder()
            .authority(uri_parts.authority.expect("request URI does not have authority set"))
            .scheme(uri_parts.scheme.expect("request URI does not have scheme set"))
            .path_and_query(path)
            .build()
            .expect("unable to construct new URI");

        (parts, body)
    } else {
        request.into_parts()
    };

    let body = match body {
        lambda_http::Body::Empty => body::empty_sync(),
        lambda_http::Body::Text(s) => body::to_boxed_sync(s),
        lambda_http::Body::Binary(v) => body::to_boxed_sync(v),
    };

    http::Request::from_parts(parts, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lambda_http::RequestExt;

    #[test]
    fn traits() {
        use crate::test_helpers::*;

        assert_send::<LambdaHandler<()>>();
        assert_sync::<LambdaHandler<()>>();
    }

    #[test]
    fn raw_http_path() {
        // lambda_http::Request doesn't have a fn `builder`
        let event = http::Request::builder()
            .uri("https://id.execute-api.us-east-1.amazonaws.com/prod/resources/1")
            .body(())
            .expect("unable to build Request");
        let (parts, _) = event.into_parts();

        // the lambda event will have a raw path which is the path without stage name in it
        let event =
            lambda_http::Request::from_parts(parts, lambda_http::Body::Empty).with_raw_http_path("/resources/1");
        let request = convert_event(event);

        assert_eq!(request.uri().path(), "/resources/1");
    }

    #[tokio::test]
    async fn body_conversion_empty() {
        let event = http::Request::builder()
            .uri("https://id.execute-api.us-east-1.amazonaws.com/test")
            .body(())
            .expect("unable to build Request");
        let (parts, _) = event.into_parts();
        let event = lambda_http::Request::from_parts(parts, lambda_http::Body::Empty);
        let request = convert_event(event);
        let bytes = crate::body::collect_bytes(request.into_body()).await.unwrap();
        assert_eq!(bytes.len(), 0);
    }

    #[tokio::test]
    async fn body_conversion_text() {
        let event = http::Request::builder()
            .uri("https://id.execute-api.us-east-1.amazonaws.com/test")
            .body(())
            .expect("unable to build Request");
        let (parts, _) = event.into_parts();
        let event = lambda_http::Request::from_parts(parts, lambda_http::Body::Text("hello world".to_string()));
        let request = convert_event(event);
        let bytes = crate::body::collect_bytes(request.into_body()).await.unwrap();
        assert_eq!(bytes, "hello world");
    }

    #[tokio::test]
    async fn body_conversion_binary() {
        let event = http::Request::builder()
            .uri("https://id.execute-api.us-east-1.amazonaws.com/test")
            .body(())
            .expect("unable to build Request");
        let (parts, _) = event.into_parts();
        let event = lambda_http::Request::from_parts(parts, lambda_http::Body::Binary(vec![1, 2, 3, 4, 5]));
        let request = convert_event(event);
        let bytes = crate::body::collect_bytes(request.into_body()).await.unwrap();
        assert_eq!(bytes.as_ref(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn uri_with_query_string() {
        let event = http::Request::builder()
            .uri("https://id.execute-api.us-east-1.amazonaws.com/prod/resources/1?foo=bar&baz=qux")
            .body(())
            .expect("unable to build Request");
        let (parts, _) = event.into_parts();
        let event =
            lambda_http::Request::from_parts(parts, lambda_http::Body::Empty).with_raw_http_path("/resources/1");
        let request = convert_event(event);

        assert_eq!(request.uri().path(), "/resources/1");
        assert_eq!(request.uri().query(), Some("foo=bar&baz=qux"));
    }

    #[test]
    fn uri_without_stage_stripping() {
        // When raw_http_path is empty or matches the path, no stripping should occur
        let event = http::Request::builder()
            .uri("https://id.execute-api.us-east-1.amazonaws.com/resources/1")
            .body(())
            .expect("unable to build Request");
        let (parts, _) = event.into_parts();
        let event = lambda_http::Request::from_parts(parts, lambda_http::Body::Empty);
        let request = convert_event(event);

        assert_eq!(request.uri().path(), "/resources/1");
    }

    #[test]
    fn headers_are_preserved() {
        let event = http::Request::builder()
            .uri("https://id.execute-api.us-east-1.amazonaws.com/test")
            .header("content-type", "application/json")
            .header("x-custom-header", "custom-value")
            .body(())
            .expect("unable to build Request");
        let (parts, _) = event.into_parts();
        let event = lambda_http::Request::from_parts(parts, lambda_http::Body::Empty);
        let request = convert_event(event);

        assert_eq!(request.headers().get("content-type").unwrap(), "application/json");
        assert_eq!(request.headers().get("x-custom-header").unwrap(), "custom-value");
    }

    #[test]
    fn extensions_are_preserved() {
        let event = http::Request::builder()
            .uri("https://id.execute-api.us-east-1.amazonaws.com/test")
            .body(())
            .expect("unable to build Request");
        let (mut parts, _) = event.into_parts();

        // Add a test extension
        #[derive(Debug, Clone, PartialEq)]
        struct TestExtension(String);
        parts.extensions.insert(TestExtension("test-value".to_string()));

        let event = lambda_http::Request::from_parts(parts, lambda_http::Body::Empty);
        let request = convert_event(event);

        let ext = request.extensions().get::<TestExtension>();
        assert!(ext.is_some());
        assert_eq!(ext.unwrap(), &TestExtension("test-value".to_string()));
    }

    #[test]
    fn method_is_preserved() {
        let event = http::Request::builder()
            .method("POST")
            .uri("https://id.execute-api.us-east-1.amazonaws.com/test")
            .body(())
            .expect("unable to build Request");
        let (parts, _) = event.into_parts();
        let event = lambda_http::Request::from_parts(parts, lambda_http::Body::Empty);
        let request = convert_event(event);

        assert_eq!(request.method(), http::Method::POST);
    }

    #[tokio::test]
    async fn lambda_handler_service_integration() {
        use tower::ServiceExt;

        // Create a simple service that echoes the URI path
        let inner_service = tower::service_fn(|req: ServiceRequest| async move {
            let path = req.uri().path().to_string();
            let response = http::Response::builder()
                .status(200)
                .body(crate::body::to_boxed(path))
                .unwrap();
            Ok::<_, std::convert::Infallible>(response)
        });

        let mut lambda_handler = LambdaHandler::new(inner_service);

        // Create a lambda request
        let event = http::Request::builder()
            .uri("https://id.execute-api.us-east-1.amazonaws.com/prod/test/path")
            .body(())
            .expect("unable to build Request");
        let (parts, _) = event.into_parts();
        let event = lambda_http::Request::from_parts(parts, lambda_http::Body::Empty).with_raw_http_path("/test/path");

        // Call the service
        let response = lambda_handler.ready().await.unwrap().call(event).await.unwrap();

        // Verify response
        assert_eq!(response.status(), 200);
        let body_bytes = crate::body::collect_bytes(response.into_body()).await.unwrap();
        assert_eq!(body_bytes, "/test/path");
    }

    #[tokio::test]
    async fn lambda_handler_with_request_body() {
        use tower::ServiceExt;

        // Create a service that processes the request body
        let inner_service = tower::service_fn(|req: ServiceRequest| async move {
            let body_bytes = crate::body::collect_bytes(req.into_body()).await.unwrap();
            let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

            let response_body = format!("Received: {}", body_str);
            let response = http::Response::builder()
                .status(200)
                .header("content-type", "text/plain")
                .body(crate::body::to_boxed(response_body))
                .unwrap();
            Ok::<_, std::convert::Infallible>(response)
        });

        let mut lambda_handler = LambdaHandler::new(inner_service);

        // Create a lambda request with JSON body
        let event = http::Request::builder()
            .method("POST")
            .uri("https://id.execute-api.us-east-1.amazonaws.com/api/process")
            .header("content-type", "application/json")
            .body(())
            .expect("unable to build Request");
        let (parts, _) = event.into_parts();
        let event = lambda_http::Request::from_parts(parts, lambda_http::Body::Text(r#"{"key":"value"}"#.to_string()));

        // Call the service
        let response = lambda_handler.ready().await.unwrap().call(event).await.unwrap();

        // Verify response
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers().get("content-type").unwrap(), "text/plain");
        let body_bytes = crate::body::collect_bytes(response.into_body()).await.unwrap();
        assert_eq!(body_bytes, r#"Received: {"key":"value"}"#);
    }

    #[tokio::test]
    async fn lambda_handler_response_headers() {
        use tower::ServiceExt;

        // Create a service that returns custom headers
        let inner_service = tower::service_fn(|_req: ServiceRequest| async move {
            let response = http::Response::builder()
                .status(201)
                .header("x-custom-header", "custom-value")
                .header("content-type", "application/json")
                .header("x-request-id", "12345")
                .body(crate::body::to_boxed(r#"{"status":"created"}"#))
                .unwrap();
            Ok::<_, std::convert::Infallible>(response)
        });

        let mut lambda_handler = LambdaHandler::new(inner_service);

        let event = http::Request::builder()
            .uri("https://id.execute-api.us-east-1.amazonaws.com/api/create")
            .body(())
            .expect("unable to build Request");
        let (parts, _) = event.into_parts();
        let event = lambda_http::Request::from_parts(parts, lambda_http::Body::Empty);

        // Call the service
        let response = lambda_handler.ready().await.unwrap().call(event).await.unwrap();

        // Verify all response components
        assert_eq!(response.status(), 201);
        assert_eq!(response.headers().get("x-custom-header").unwrap(), "custom-value");
        assert_eq!(response.headers().get("content-type").unwrap(), "application/json");
        assert_eq!(response.headers().get("x-request-id").unwrap(), "12345");

        let body_bytes = crate::body::collect_bytes(response.into_body()).await.unwrap();
        assert_eq!(body_bytes, r#"{"status":"created"}"#);
    }

    #[tokio::test]
    async fn lambda_handler_error_response() {
        use tower::ServiceExt;

        // Create a service that returns an error status
        let inner_service = tower::service_fn(|_req: ServiceRequest| async move {
            let response = http::Response::builder()
                .status(404)
                .header("content-type", "application/json")
                .body(crate::body::to_boxed(r#"{"error":"not found"}"#))
                .unwrap();
            Ok::<_, std::convert::Infallible>(response)
        });

        let mut lambda_handler = LambdaHandler::new(inner_service);

        let event = http::Request::builder()
            .uri("https://id.execute-api.us-east-1.amazonaws.com/api/missing")
            .body(())
            .expect("unable to build Request");
        let (parts, _) = event.into_parts();
        let event = lambda_http::Request::from_parts(parts, lambda_http::Body::Empty);

        // Call the service
        let response = lambda_handler.ready().await.unwrap().call(event).await.unwrap();

        // Verify error response
        assert_eq!(response.status(), 404);
        let body_bytes = crate::body::collect_bytes(response.into_body()).await.unwrap();
        assert_eq!(body_bytes, r#"{"error":"not found"}"#);
    }
}
