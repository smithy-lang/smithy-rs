/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP WASI Adapter

use aws_smithy_client::{erase::DynConnector, http_connector::HttpConnector};
use aws_smithy_http::{body::SdkBody, result::ConnectorError};
use bytes::Bytes;
use http::{Request, Response};
use std::{
    sync::Arc,
    task::{Context, Poll},
};
use tower::Service;
use wasi_preview2_prototype::http_client::DefaultClient;

/// Creates a connector function that can be used during instantiation of the client SDK
/// in order to route the HTTP requests through the WebAssembly host. The host must
/// support the WASI HTTP proposal as defined in the Preview 2 specification.
pub fn wasi_connector() -> HttpConnector {
    HttpConnector::ConnectorFn(Arc::new(|_, _| Some(DynConnector::new(Adapter::default()))))
}

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
        tracing::debug!("adapter: sending request");
        tracing::trace!("request details {:?}", req);
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
            tracing::debug!("adapter: response received");
            tracing::trace!("response details {:?}", res);

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
