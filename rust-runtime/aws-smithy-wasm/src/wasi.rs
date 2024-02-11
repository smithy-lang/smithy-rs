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
        result::{ConnectorError, SdkError},
        runtime_components::RuntimeComponents,
    },
    http::Response,
    shared::IntoShared,
};
use aws_smithy_types::body::SdkBody;
use bytes::Bytes;
// use crate::wasi::{outgoing_handler, types};
use http::{HeaderName, HeaderValue};
use wasi::http::{outgoing_handler, types as http_types};
use wasi::io::poll;
use wasi::io::streams;
// use wasi_preview2_prototype::http_client::DefaultClient;
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

        // Right now only synchronous calls can be made through WASI
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
            // .expect("response from adapter");

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

        let res = outgoing_handler::handle(req.id, self.options).expect("Http response");
        let subscription = res.subscribe();
        subscription.block();

        //This is pretty ugly because the FutureIncomingResponse .get() method returns a
        //Option<Result<Result<IncomingResponse, ErrorCode>, ()>>.
        //The outer Option is the readiness which we know is Some because we .block() waiting for it
        //The outer Result is just a singleton enforcer so we can only get the response once
        //The inner Result indicates whether the HTTP call was sent/received successfully (not the 200 succes of the call)
        let incoming_res = res
            .get()
            .unwrap()
            .unwrap()
            .map_err(|err| ConnectorError::other(err.into(), None))?;

        let response =
            http::Response::try_from(WasiResponse(incoming_res)).expect("Converting http response");

        Ok(response)
    }
}

#[derive(Debug)]
pub struct WasiRequest {
    id: outgoing_handler::OutgoingRequest,
    body: http_types::OutputStream,
}

impl WasiRequest {
    pub fn new(id: outgoing_handler::OutgoingRequest, body: http_types::OutputStream) -> Self {
        Self { id, body }
    }
}

impl TryFrom<http::Request<Bytes>> for WasiRequest {
    type Error = ParseError;

    fn try_from(value: http::Request<Bytes>) -> Result<Self, Self::Error> {
        let (parts, body) = value.into_parts();
        let method = WasiMethod::try_from(parts.method)
            .map_err(|_| ParseError::new("Invalid http Method"))?;
        let path_with_query = parts.uri.path_and_query();
        let headers = WasiHeaders::from(&parts.headers);
        let scheme = match parts.uri.scheme_str().unwrap_or("") {
            "http" => Some(&http_types::Scheme::Http),
            "https" => Some(&http_types::Scheme::Https),
            _ => None,
        };
        let request = http_types::new_outgoing_request(
            &method,
            path_with_query.map(|q| q.as_str()),
            scheme,
            parts.uri.authority().map(|a| a.as_str()),
            headers.to_owned(),
        );

        let request2 = http_types::OutgoingRequest::new(&parts.headers);

        let request_body = http_types::outgoing_request_write(request)
            .map_err(|_| ParseError::new("outgoing request write failed"))?;

        if body.is_empty() {
            let pollable = streams::subscribe_to_output_stream(request_body);
            let mut buf = body.as_ref();
            while !buf.is_empty() {
                poll::poll_oneoff(&[pollable]);

                let permit = match streams::check_write(request_body) {
                    Ok(n) => usize::try_from(n)?,
                    Err(_) => return Err(ParseError::new("Output stream error")),
                };

                let len = buf.len().min(permit);
                let (chunk, rest) = buf.split_at(len);
                buf = rest;

                if streams::write(request_body, chunk).is_err() {
                    return Err(ParseError::new("Output stream error"));
                }
            }

            if streams::flush(request_body).is_err() {
                return Err(ParseError::new("Output stream error"));
            }

            poll::poll_oneoff(&[sub.pollable]);

            if streams::check_write(request_body).is_err() {
                return Err(ParseError::new("Output stream error"));
            }
        }

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
        let future_response = value;

        let incoming_response = match http_types::future_incoming_response_get(future_response) {
            Some(result) => result,
            None => {
                let pollable = http_types::listen_to_future_incoming_response(future_response);
                let _ = poll::poll_oneoff(&[pollable]);
                http_types::future_incoming_response_get(future_response)
                    .expect("incoming response available")
            }
        }
        .map_err(|e| ParseError::new("incoming response error: {e:?}"))?;

        let status = http_types::incoming_response_status(incoming_response);

        let body_stream = http_types::incoming_response_consume(incoming_response)
            .map_err(|_| ParseError::new("consuming incoming response"))?;

        let mut body = BytesMut::new();
        {
            let pollable = streams::subscribe_to_input_stream(body_stream);
            poll::poll_oneoff(&[pollable]);
            let mut eof = streams::StreamStatus::Open;
            while eof != streams::StreamStatus::Ended {
                let (body_chunk, stream_status) = streams::read(body_stream, u64::MAX)
                    .map_err(|e| ParseError::new("reading response body: {e:?}"))?;
                eof = if body_chunk.is_empty() {
                    streams::StreamStatus::Ended
                } else {
                    stream_status
                };
                body.put(body_chunk.as_slice());
            }
        }

        let mut res = http::Response::builder()
            .status(status)
            .body(body.freeze())
            .map_err(|_| ParseError::new("building http response"))?;

        let headers_handle = http_types::incoming_response_headers(incoming_response);
        if headers_handle > 0 {
            let headers_map = res.headers_mut();
            for (name, value) in http_types::fields_entries(headers_handle) {
                headers_map.insert(
                    HeaderName::from_bytes(name.as_bytes())
                        .map_err(|_| ParseError::new("converting response header name"))?,
                    HeaderValue::from_bytes(value.as_slice())
                        .map_err(|_| ParseError::new("converting response header value"))?,
                );
            }
        }

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

impl<'a> From<&'a http::HeaderMap> for WasiHeaders {
    fn from(headers: &'a http::HeaderMap) -> Self {
        let entries = headers
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    value.to_str().unwrap().as_bytes().to_vec(),
                )
            })
            .collect::<Vec<_>>()
            .as_slice();

        //The fields should come from the SDK so we trust that they are valid
        let fields = http_types::Fields::from_list(entries).expect("Invalid http headers.");

        Self(fields)
    }
}
