/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use http::uri;
use lambda_http::{Request, RequestExt};
use std::{
    fmt::Debug,
    task::{Context, Poll},
};
use tower::Service;

type HyperRequest = http::Request<hyper::Body>;

/// A [`MakeService`] that produces AWS Lambda compliant services.
///
/// [`MakeService`]: tower::make::MakeService
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

fn convert_event(request: Request) -> HyperRequest {
    let raw_path = request.raw_http_path();
    let (mut parts, body) = request.into_parts();
    let mut path = String::from(parts.uri.path());

    if !raw_path.is_empty() && raw_path != path {
        path = raw_path;

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
    }

    let body = match body {
        lambda_http::Body::Empty => hyper::Body::empty(),
        lambda_http::Body::Text(s) => hyper::Body::from(s),
        lambda_http::Body::Binary(v) => hyper::Body::from(v),
    };

    http::Request::from_parts(parts, body)
}
