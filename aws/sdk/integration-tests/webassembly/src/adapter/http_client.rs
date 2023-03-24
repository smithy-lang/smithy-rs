/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(unused_variables)]

use aws_smithy_http::body::SdkBody;
use http::header::{HeaderName, HeaderValue};
use std::str::FromStr;
use wasi::{default_outgoing_http, poll, streams, types as http_types};

pub(crate) fn make_request(req: http::Request<SdkBody>) -> anyhow::Result<http::Response<SdkBody>> {
    let options = Some(http_types::RequestOptions {
        connect_timeout_ms: None,
        first_byte_timeout_ms: None,
        between_bytes_timeout_ms: None,
    });
    let req = HttpRequest::from(req).inner();
    // println!("HttpRequest: {:?}", req);
    let res = default_outgoing_http::handle(req, options);
    println!("{:?}", res);
    let res: http::Response<SdkBody> = HttpResponse(res).into();
    http_types::drop_outgoing_request(req);
    Ok(res)
}

pub struct HttpRequest(http_types::OutgoingRequest);

impl HttpRequest {
    pub fn inner(self) -> http_types::OutgoingRequest {
        self.0
    }
}

impl From<http::Request<SdkBody>> for HttpRequest {
    fn from(req: http::Request<SdkBody>) -> Self {
        let (parts, sdk_body) = req.into_parts();
        // println!("request parts: {:?}", parts);
        // println!("request body: {:?}", sdk_body);
        let path = parts.uri.path();
        let query = parts.uri.query();
        let method = HttpMethod::from(parts.method);
        let headers = HttpHeader::from(&parts.headers);
        let scheme = match parts.uri.scheme_str().unwrap_or("") {
            "http" => Some(http_types::SchemeParam::Http),
            "https" => Some(http_types::SchemeParam::Https),
            _ => None,
        };
        Self(http_types::new_outgoing_request(
            method.inner(),
            path,
            query.unwrap_or(""),
            scheme,
            parts.uri.authority().map(|a| a.as_str()).unwrap(),
            headers.inner(),
        ))
    }
}

pub struct HttpMethod<'a>(http_types::MethodParam<'a>);

impl<'a> HttpMethod<'a> {
    pub fn inner(self) -> http_types::MethodParam<'a> {
        self.0
    }
}

impl<'a> From<http::Method> for HttpMethod<'a> {
    fn from(method: http::Method) -> Self {
        Self(match method {
             http::Method::GET => http_types::MethodParam::Get,
             http::Method::POST => http_types::MethodParam::Post,
             http::Method::PUT => http_types::MethodParam::Put,
             http::Method::DELETE => http_types::MethodParam::Delete,
             http::Method::PATCH => http_types::MethodParam::Patch,
             http::Method::CONNECT => http_types::MethodParam::Connect,
             http::Method::TRACE => http_types::MethodParam::Trace,
             http::Method::HEAD => http_types::MethodParam::Head,
             http::Method::OPTIONS => http_types::MethodParam::Options,
             _ => panic!("failed due to unsupported method, currently supported methods are: GET, POST, PUT, DELETE, PATCH, CONNECT, TRACE, HEAD, and OPTIONS"),
         })
    }
}

struct HttpResponse(http_types::IncomingResponse);

impl HttpResponse {
    pub fn inner(self) -> http_types::IncomingResponse {
        self.0
    }
}

impl From<HttpResponse> for http::Response<SdkBody> {
    fn from(val: HttpResponse) -> Self {
        let res_pointer = val.inner();
        poll::drop_pollable(res_pointer);
        let status = http_types::incoming_response_status(res_pointer);
        println!("status: {}", status);
        let header_handle = http_types::incoming_response_headers(res_pointer);
        let headers = http_types::fields_entries(header_handle);
        println!("headers: {:?}", headers);
        let stream: http_types::IncomingStream =
            http_types::incoming_response_consume(res_pointer).unwrap();
        let len = 64 * 1024;
        let (body, finished) = streams::read(stream, len).unwrap();
        let body = if body.is_empty() {
            SdkBody::empty()
        } else {
            SdkBody::from(body)
        };
        let mut res = http::Response::builder().status(status).body(body).unwrap();
        let headers_map = res.headers_mut();
        for (name, value) in headers {
            headers_map.insert(
                HeaderName::from_str(name.as_ref()).unwrap(),
                HeaderValue::from_str(value.as_str()).unwrap(),
            );
        }
        streams::drop_input_stream(stream);
        http_types::drop_incoming_response(res_pointer);
        res
    }
}

pub struct HttpHeader(http_types::Fields);

impl HttpHeader {
    pub fn inner(self) -> http_types::Fields {
        self.0
    }
}

impl<'a> From<&'a http::HeaderMap> for HttpHeader {
    fn from(headers: &'a http::HeaderMap) -> Self {
        Self(http_types::new_fields(
            &headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.to_str().unwrap()))
                .collect::<Vec<(&'a str, &'a str)>>(),
        ))
    }
}
