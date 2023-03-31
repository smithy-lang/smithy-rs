/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP Service Adapter used in WASI environment

use aws_smithy_http::body::SdkBody;
use aws_smithy_http::result::ConnectorError;
use http::{Request, Response};
use std::task::{Context, Poll};
use tower::Service;

#[derive(Default, Debug, Clone)]
pub(crate) struct Adapter {}

impl Service<Request<SdkBody>> for Adapter {
    type Response = Response<SdkBody>;

    type Error = ConnectorError;

    #[allow(clippy::type_complexity)]
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>,
    >;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<SdkBody>) -> Self::Future {
        println!("Adapter: sending request...");
        let client = wasi_http::DefaultClient::new(None);
        let (parts, body) = req.into_parts();
        let body = match body.bytes() {
            Some(value) => value.to_vec(),
            None => Vec::new(),
        };
        let req = Request::from_parts(parts, body);
        let res = client.handle(req).unwrap();
        let (parts, body) = res.into_parts();
        let body = if body.is_empty() {
            SdkBody::empty()
        } else {
            SdkBody::from(body)
        };
        Box::pin(async move { Ok(Response::<SdkBody>::from_parts(parts, body)) })
    }
}
