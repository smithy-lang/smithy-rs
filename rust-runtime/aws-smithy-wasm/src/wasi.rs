/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP WASI Adapter

use aws_smithy_http::{body::SdkBody, result::ConnectorError};
use aws_smithy_runtime_api::{
    client::{
        http::{
            HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings,
            SharedHttpClient, SharedHttpConnector,
        },
        orchestrator::HttpRequest,
        runtime_components::RuntimeComponents,
    },
    shared::IntoShared,
};
use bytes::Bytes;
use wasi_preview2_prototype::http_client::DefaultClient;

/// Creates a connector function that can be used during instantiation of the client SDK
/// in order to route the HTTP requests through the WebAssembly host. The host must
/// support the WASI HTTP proposal as defined in the Preview 2 specification.
pub fn wasi_http_client() -> SharedHttpClient {
    WasiHttpClient::new().into_shared()
}

/// HTTP client used in WASI environment
#[derive(Debug, Clone)]
pub struct WasiHttpClient {
    connector: SharedHttpConnector,
}

impl WasiHttpClient {
    /// Create a new Wasi HTTP client.
    pub fn new() -> Self {
        Default::default()
    }
}

impl Default for WasiHttpClient {
    fn default() -> Self {
        Self {
            connector: WasiHttpConnector.into_shared(),
        }
    }
}

impl HttpClient for WasiHttpClient {
    fn http_connector(
        &self,
        _settings: &HttpConnectorSettings,
        _components: &RuntimeComponents,
    ) -> SharedHttpConnector {
        // TODO(wasi): add connect/read timeouts
        self.connector.clone()
    }
}

/// HTTP connector used in WASI environment
#[non_exhaustive]
#[derive(Debug)]
pub struct WasiHttpConnector;

impl HttpConnector for WasiHttpConnector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        tracing::trace!("WasiHttpConnector: sending request {request:?}");
        let client = DefaultClient::new(None);
        // Right now only synchronous calls can be made through WASI
        let fut = client.handle(request.map(|body| match body.bytes() {
            Some(value) => Bytes::copy_from_slice(value),
            None => Bytes::new(),
        }));
        HttpConnectorFuture::new(async move {
            let response = fut
                .map_err(|err| ConnectorError::other(err.into(), None))
                .expect("response from adapter");
            tracing::trace!("WasiHttpConnector: response received {response:?}");
            let (parts, body) = response.into_parts();
            let loaded_body = if body.is_empty() {
                SdkBody::empty()
            } else {
                SdkBody::from(body)
            };
            Ok(http::Response::from_parts(parts, loaded_body))
        })
    }
}
