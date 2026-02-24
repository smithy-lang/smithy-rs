/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[cfg(not(feature = "aws-smithy-http-server-065"))]
pub(crate) type RequestBody = hyper::body::Incoming;
#[cfg(not(feature = "aws-smithy-http-server-065"))]
pub(crate) use aws_smithy_http_server;

#[cfg(feature = "aws-smithy-http-server-065")]
pub(crate) use aws_smithy_http_server_065 as aws_smithy_http_server;
#[cfg(feature = "aws-smithy-http-server-065")]
use http_02 as http;
#[cfg(feature = "aws-smithy-http-server-065")]
pub(crate) type RequestBody = hyper_014::Body;

pub(crate) type ResponseBody = aws_smithy_http_server::body::BoxBody;
pub type HttpRequest = http::Request<RequestBody>; // pub because needed in macro
pub(crate) type HttpResponse = http::Response<ResponseBody>;
pub(crate) type HttpRequestParts = http::request::Parts;
pub(crate) type HttpStatusCode = http::StatusCode;

pub(crate) type DefaultInit<Entry, Sink> =
    fn(&mut HttpRequest) -> metrique::AppendAndCloseOnDrop<Entry, Sink>;
pub(crate) type DefaultRs<Entry> = fn(&mut HttpResponse, &mut Entry);

pub(crate) fn empty_response_body() -> ResponseBody {
    #[cfg(not(feature = "aws-smithy-http-server-065"))]
    return aws_smithy_http_server::body::empty();
    #[cfg(feature = "aws-smithy-http-server-065")]
    return aws_smithy_http_server::body::boxed(http_body_04::Empty::new());
}
