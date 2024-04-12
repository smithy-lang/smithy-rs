/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use http::uri;
use lambda_http::{Request, RequestExt};
use std::{
    fmt::Debug,
    task::{Context, Poll},
};
use tower::Service;

type HyperRequest = http::Request<hyper::Body>;

/// A [`Service`] that takes a `lambda_http::Request` and converts
/// it to `http::Request<hyper::Body>`.
///
/// **This version is only guaranteed to be compatible with
/// [`lambda_http`](https://docs.rs/lambda_http) ^0.7.0.** Please ensure that your service crate's
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
    S: Service<HyperRequest>,
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

/// Converts a `lambda_http::Request` into a `http::Request<hyper::Body>`
/// Issue: <https://github.com/smithy-lang/smithy-rs/issues/1125>
///
/// While converting the event the [API Gateway Stage] portion of the URI
/// is removed from the uri that gets returned as a new `http::Request`.
///
/// [API Gateway Stage]: https://docs.aws.amazon.com/apigateway/latest/developerguide/http-api-stages.html
fn convert_event(request: Request) -> HyperRequest {
    let raw_path: &str = request.extensions().raw_http_path();
    let path: &str = request.uri().path();

    let (parts, body) = if !raw_path.is_empty() && raw_path != path {
        let mut path = raw_path.to_owned(); // Clone only when we need to strip out the stage.
        let (mut parts, body) = request.into_parts();

        let uri_parts: uri::Parts = parts.uri.into();
        let path_and_query = uri_parts
            .path_and_query
            .expect("request URI does not have `PathAndQuery`");

        if let Some(query) = path_and_query.query() {
            path.push('?');
            path.push_str(query);
        }

        parts.uri = uri::Uri::builder()
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
        lambda_http::Body::Empty => hyper::Body::empty(),
        lambda_http::Body::Text(s) => hyper::Body::from(s),
        lambda_http::Body::Binary(v) => hyper::Body::from(v),
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

        assert_eq!(request.uri().path(), "/resources/1")
    }
}
