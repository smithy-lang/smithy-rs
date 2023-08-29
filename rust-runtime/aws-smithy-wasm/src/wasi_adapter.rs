/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP WASI Adapter

use aws_smithy_http::{body::SdkBody, byte_stream::ByteStream, result::ConnectorError};
use bytes::Bytes;
use http::{Request, Response};
use std::task::{Context, Poll};
use tower::Service;
use wasi_preview2_prototype::http_client::DefaultClient;

#[derive(Default, Debug, Clone)]
/// HTTP Service Adapter used in WASI environment
pub struct Adapter {}

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

    fn call(&mut self, req: http::Request<SdkBody>) -> Self::Future {
        println!("Adapter: sending request...");
        let client = DefaultClient::new(None);
        // Right now only synchronous calls can be made through WASI
        let fut = client.handle(req.map(|body| match body.bytes() {
            Some(value) => Bytes::copy_from_slice(value),
            None => Bytes::new(),
        }));
        Box::pin(async move {
            let res = fut
                .map_err(|err| ConnectorError::other(err.into(), None))
                .expect("response from adapter");

            let (parts, body) = res.into_parts();
            let loaded_body = if body.is_empty() {
                SdkBody::empty()
            } else {
                SdkBody::from(body)
            };
            Ok(http::Response::from_parts(parts, loaded_body))
        })
    }
}
