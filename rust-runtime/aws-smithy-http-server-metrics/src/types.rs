/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub(crate) type RequestBody = hyper::body::Incoming;

pub(crate) use aws_smithy_http_server;

pub(crate) type ResponseBody = aws_smithy_http_server::body::BoxBody;
pub type HttpRequest = http::Request<RequestBody>; // pub because needed in macro
pub(crate) type HttpResponse = http::Response<ResponseBody>;
pub(crate) type HttpRequestParts = http::request::Parts;
pub(crate) type HttpStatusCode = http::StatusCode;

pub(crate) type DefaultInit<Entry, Sink> =
    fn(&mut HttpRequest) -> metrique::AppendAndCloseOnDrop<Entry, Sink>;
pub(crate) type DefaultRs<Entry> = fn(&mut HttpResponse, &mut Entry);

pub(crate) fn empty_response_body() -> ResponseBody {
    aws_smithy_http_server::body::empty()
}
