/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP WASI Adapter

use std::ops::Deref;

use aws_smithy_http::header::ParseError;
use aws_smithy_runtime_api::{
    client::{
        http::{
            HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings,
            SharedHttpClient, SharedHttpConnector,
        },
        orchestrator::HttpRequest,
        result::ConnectorError,
        runtime_components::RuntimeComponents,
    },
    http::Response,
    shared::IntoShared,
};
use aws_smithy_types::body::SdkBody;
use bytes::{Bytes, BytesMut};
use wasi::http::{
    outgoing_handler,
    types::{self as http_types, RequestOptions},
};

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
        let client = WasiDefaultClient::new(None);
        let http_req = request.try_into_http1x().expect("http request invalid");
        let converted_req = http_req.map(|body| match body.bytes() {
            Some(value) => Bytes::copy_from_slice(value),
            None => Bytes::new(),
        });

        let fut_result = client.handle(converted_req);

        HttpConnectorFuture::new(async move {
            let fut = fut_result?;
            let response = fut.map(|body| {
                if body.is_empty() {
                    SdkBody::empty()
                } else {
                    SdkBody::from(body)
                }
            });
            tracing::trace!("WasiHttpConnector: response received {response:?}");

            let sdk_res = Response::try_from(response)
                .map_err(|err| ConnectorError::other(err.into(), None))?;

            Ok(sdk_res)
        })
    }
}

pub struct WasiDefaultClient {
    options: Option<outgoing_handler::RequestOptions>,
}

impl WasiDefaultClient {
    pub fn new(options: Option<outgoing_handler::RequestOptions>) -> Self {
        Self { options }
    }

    pub fn handle(
        &self,
        req: http::Request<Bytes>,
    ) -> Result<http::Response<Bytes>, ConnectorError> {
        let req = WasiRequest::try_from(req).expect("Converting http request");

        //RequestOptions doesn't impl Clone or Copy and outgoing_handler::handle takes ownership,
        //so we need to recreate it
        let options = if let Some(opts) = &self.options {
            let new_opts = RequestOptions::new();
            new_opts.set_between_bytes_timeout(opts.between_bytes_timeout());
            new_opts.set_connect_timeout(opts.connect_timeout());
            new_opts.set_first_byte_timeout(opts.first_byte_timeout());

            Some(new_opts)
        } else {
            None
        };

        let res = outgoing_handler::handle(req.id, options).expect("Http response");
        // Right now only synchronous calls can be made through WASI, so we subscribe and
        // block on the FutureIncomingResponse
        let subscription = res.subscribe();
        subscription.block();

        //The FutureIncomingResponse .get() method returns a
        //Option<Result<Result<IncomingResponse, ErrorCode>, ()>>.
        //The outer Option ensures readiness which we know is Some because we .block() waiting for it
        //The outer Result is just a singleton enforcer so we can only get the response once
        //The inner Result indicates whether the HTTP call was sent/received successfully (not the 200 succes of the call)
        let incoming_res = res
            .get()
            .expect("http response not ready")
            .expect("http response accessed more than once")
            .map_err(|err| ConnectorError::other(err.into(), None))?;

        let response =
            http::Response::try_from(WasiResponse(incoming_res)).expect("Converting http response");

        Ok(response)
    }
}

#[derive(Debug)]
pub struct WasiRequest {
    id: outgoing_handler::OutgoingRequest,
    body: http_types::OutgoingBody,
}

impl WasiRequest {
    pub fn new(id: outgoing_handler::OutgoingRequest, body: http_types::OutgoingBody) -> Self {
        Self { id, body }
    }
}

impl TryFrom<http::Request<Bytes>> for WasiRequest {
    type Error = ParseError;

    fn try_from(value: http::Request<Bytes>) -> Result<Self, Self::Error> {
        let (parts, body) = value.into_parts();
        let method = WasiMethod::try_from(parts.method)
            .map_err(|_| ParseError::new("Invalid http Method"))?;
        let path_with_query = parts.uri.path_and_query().map(|path| path.as_str());
        let headers = WasiHeaders::from(parts.headers);
        let scheme = match parts.uri.scheme_str().unwrap_or("") {
            "http" => Some(&http_types::Scheme::Http),
            "https" => Some(&http_types::Scheme::Https),
            _ => None,
        };
        let authority = parts.uri.authority().map(|auth| auth.as_str());

        let request = http_types::OutgoingRequest::new(headers.0);
        request.set_scheme(scheme);
        request.set_method(&method);
        request.set_path_with_query(path_with_query);
        request.set_authority(authority);

        let request_body = request.body().expect("body accessed more than once");

        let request_stream = request_body
            .write()
            .expect("output stream accessed more than once");

        request_stream
            .blocking_write_and_flush(&body)
            .expect("failed to write body");

        //The OutputStream is a child resource: it must be dropped
        //before the parent OutgoingBody resource is dropped (or finished),
        //otherwise the OutgoingBody drop or finish will trap.
        drop(request_stream);

        Ok(WasiRequest::new(request, request_body))
    }
}

pub struct WasiMethod(http_types::Method);

impl Deref for WasiMethod {
    type Target = http_types::Method;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<http::Method> for WasiMethod {
    type Error = ParseError;

    fn try_from(method: http::Method) -> Result<Self, Self::Error> {
        Ok(Self(match method {
            http::Method::GET => http_types::Method::Get,
            http::Method::POST => http_types::Method::Post,
            http::Method::PUT => http_types::Method::Put,
            http::Method::DELETE => http_types::Method::Delete,
            http::Method::PATCH => http_types::Method::Patch,
            http::Method::CONNECT => http_types::Method::Connect,
            http::Method::TRACE => http_types::Method::Trace,
            http::Method::HEAD => http_types::Method::Head,
            http::Method::OPTIONS => http_types::Method::Options,
            _ => return Err(ParseError::new("failed due to unsupported method, currently supported methods are: GET, POST, PUT, DELETE, PATCH, CONNECT, TRACE, HEAD, and OPTIONS")),
        }))
    }
}

pub struct WasiResponse(http_types::IncomingResponse);

impl Deref for WasiResponse {
    type Target = http_types::IncomingResponse;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<WasiResponse> for http::Response<Bytes> {
    type Error = ParseError;

    fn try_from(value: WasiResponse) -> Result<Self, Self::Error> {
        let response = value.0;

        let status = response.status();

        let body_stream = response
            .consume()
            .expect("consume called more than once")
            .stream()
            .expect("stream accessed more than once");

        let mut body = BytesMut::new();

        //blocking_read blocks until at least one byte is available to read
        while let Ok(stream_bytes) = body_stream.blocking_read(u64::MAX) {
            body.extend_from_slice(stream_bytes.as_slice())
        }

        let headers = response.headers().entries();

        let res_build = headers
            .into_iter()
            .fold(http::Response::builder().status(status), |rb, header| {
                rb.header(header.0, header.1)
            });

        let res = res_build
            .body(body.freeze())
            .map_err(|_| ParseError::new("building http response"))?;

        Ok(res)
    }
}

pub struct WasiHeaders(http_types::Fields);

impl Deref for WasiHeaders {
    type Target = http_types::Fields;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<http::HeaderMap> for WasiHeaders {
    fn from(headers: http::HeaderMap) -> Self {
        let entries = headers
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    value.to_str().unwrap().as_bytes().to_vec(),
                )
            })
            .collect::<Vec<_>>();

        let fields = http_types::Fields::from_list(&entries).expect("Invalid http headers.");

        Self(fields)
    }
}
