/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP Service Adapter used in WASI environment

use aws_smithy_http::body::SdkBody;
use aws_smithy_http::result::ConnectorError;
use bytes::Bytes;
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
        // Right now only synchronous calls can be made through WASI
        let fut = client.handle(req.map(|body| match body.bytes() {
            Some(value) => Bytes::copy_from_slice(value),
            None => Bytes::new(),
        }));
        Box::pin(async move {
            Ok(fut
                .map_err(|err| ConnectorError::other(err.into(), None))?
                .map(SdkBody::from))
        })
    }
}
