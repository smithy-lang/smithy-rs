/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP WASI Adapter
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
    types::{self as wasi_http, OutgoingBody, RequestOptions},
};

/// Creates a HTTP client that can be used during instantiation of the client SDK
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
        // TODO(wasi): add connect/read timeouts
        let client = WasiDefaultClient::new(WasiRequestOptions(None));
        let http_req = request.try_into_http1x().expect("Http request invalid");
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

/// Wasi http client containing the options to pass to the
struct WasiDefaultClient {
    options: WasiRequestOptions,
}

impl WasiDefaultClient {
    /// Create a new Wasi HTTP client.
    fn new(options: WasiRequestOptions) -> Self {
        Self { options }
    }

    /// Make outgoing http request in a Wasi environment
    fn handle(&self, req: http::Request<Bytes>) -> Result<http::Response<Bytes>, ConnectorError> {
        let req = WasiRequest::try_from(req).expect("Converting http request");

        let res = outgoing_handler::handle(req.0, self.options.clone().0).expect("Http response");

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
            .expect("Http response not ready")
            .expect("Http response accessed more than once")
            .map_err(|err| ConnectorError::other(err.into(), None))?;

        let response = http::Response::try_from(WasiResponse(incoming_res))
            .expect("Converting to http response");

        Ok(response)
    }
}

/// Wrapper for the Wasi RequestOptions type to allow us to impl Clone
struct WasiRequestOptions(Option<outgoing_handler::RequestOptions>);

//The Wasi RequestOptions type doesn't impl copy or clone but the outgoing_handler::handle method
//takes ownership, so we impl it on this wrapper type
impl Clone for WasiRequestOptions {
    fn clone(&self) -> Self {
        let new_opts = if let Some(opts) = &self.0 {
            let new_opts = RequestOptions::new();
            new_opts
                .set_between_bytes_timeout(opts.between_bytes_timeout())
                .expect("Between bytes timeout");
            new_opts
                .set_connect_timeout(opts.connect_timeout())
                .expect("Connect timeout");
            new_opts
                .set_first_byte_timeout(opts.first_byte_timeout())
                .expect("First byte timeout");

            Some(new_opts)
        } else {
            None
        };

        Self(new_opts)
    }
}

/// Wrapper to allow converting between http Request types and Wasi Request types
#[derive(Debug)]
struct WasiRequest(outgoing_handler::OutgoingRequest);

impl WasiRequest {
    fn new(req: outgoing_handler::OutgoingRequest) -> Self {
        Self(req)
    }
}

impl TryFrom<http::Request<Bytes>> for WasiRequest {
    type Error = ParseError;

    fn try_from(value: http::Request<Bytes>) -> Result<Self, Self::Error> {
        let (parts, body) = value.into_parts();
        let method =
            WasiMethod::try_from(parts.method).map_err(|err| ParseError::new(err.to_string()))?;
        let path_with_query = parts.uri.path_and_query().map(|path| path.as_str());
        let headers = WasiHeaders::from(parts.headers);
        let scheme = match parts.uri.scheme_str().unwrap_or("") {
            "http" => Some(&wasi_http::Scheme::Http),
            "https" => Some(&wasi_http::Scheme::Https),
            _ => None,
        };
        let authority = parts.uri.authority().map(|auth| auth.as_str());

        let request = wasi_http::OutgoingRequest::new(headers.0);
        request.set_scheme(scheme).expect("Set scheme");
        request.set_method(&method.0).expect("Set method");
        request
            .set_path_with_query(path_with_query)
            .expect("Set path with query");
        request.set_authority(authority).expect("Set authority");

        let request_body = request.body().expect("Body accessed more than once");

        let request_stream = request_body
            .write()
            .expect("Output stream accessed more than once");

        request_stream
            .blocking_write_and_flush(&body)
            .expect("Failed to write body");

        //The OutputStream is a child resource: it must be dropped
        //before the parent OutgoingBody resource is dropped (or finished),
        //otherwise the OutgoingBody drop or finish will trap.
        drop(request_stream);

        OutgoingBody::finish(request_body, None).expect("Http body finished");

        Ok(WasiRequest::new(request))
    }
}

/// Wrapper to allow converting between http Methods and Wasi Methods
struct WasiMethod(wasi_http::Method);

impl TryFrom<http::Method> for WasiMethod {
    type Error = ParseError;

    fn try_from(method: http::Method) -> Result<Self, Self::Error> {
        Ok(Self(match method {
            http::Method::GET => wasi_http::Method::Get,
            http::Method::POST => wasi_http::Method::Post,
            http::Method::PUT => wasi_http::Method::Put,
            http::Method::DELETE => wasi_http::Method::Delete,
            http::Method::PATCH => wasi_http::Method::Patch,
            http::Method::CONNECT => wasi_http::Method::Connect,
            http::Method::TRACE => wasi_http::Method::Trace,
            http::Method::HEAD => wasi_http::Method::Head,
            http::Method::OPTIONS => wasi_http::Method::Options,
            _ => return Err(ParseError::new("failed due to unsupported method, currently supported methods are: GET, POST, PUT, DELETE, PATCH, CONNECT, TRACE, HEAD, and OPTIONS")),
        }))
    }
}

/// Wrapper to allow converting between http Response types and Wasi Response types
struct WasiResponse(wasi_http::IncomingResponse);

impl TryFrom<WasiResponse> for http::Response<Bytes> {
    type Error = ParseError;

    fn try_from(value: WasiResponse) -> Result<Self, Self::Error> {
        let response = value.0;

        let status = response.status();

        //This headers resource is a child: it must be dropped before the parent incoming-response is dropped.
        //The drop happens via the consuming iterator used below
        let headers = response.headers().entries();

        let res_build = headers
            .into_iter()
            .fold(http::Response::builder().status(status), |rb, header| {
                rb.header(header.0, header.1)
            });

        let body_incoming = response.consume().expect("Consume called more than once");

        //The input-stream resource is a child: it must be dropped before the parent
        //incoming-body is dropped, or consumed by incoming-body.finish.
        //That drop is done explicitly below
        let body_stream = body_incoming
            .stream()
            .expect("Stream accessed more than once");

        let mut body = BytesMut::new();

        //blocking_read blocks until at least one byte is available
        while let Ok(stream_bytes) = body_stream.blocking_read(u64::MAX) {
            body.extend_from_slice(stream_bytes.as_slice())
        }

        drop(body_stream);

        let res = res_build
            .body(body.freeze())
            .map_err(|err| ParseError::new(err.to_string()))?;

        Ok(res)
    }
}

/// Wrapper to allow converting between http headers and Wasi headers
struct WasiHeaders(wasi_http::Fields);

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

        let fields = wasi_http::Fields::from_list(&entries).expect("Invalid http headers.");

        Self(fields)
    }
}
