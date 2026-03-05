/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[cfg(not(any(
    feature = "aws-smithy-http-server-065",
    feature = "aws-smithy-legacy-http-server"
)))]
pub(crate) type RequestBody = hyper::body::Incoming;
#[cfg(not(any(
    feature = "aws-smithy-http-server-065",
    feature = "aws-smithy-legacy-http-server"
)))]
pub(crate) use aws_smithy_http_server;

/// For pre-http1x codegen version support
#[cfg(all(
    feature = "aws-smithy-http-server-065",
    not(feature = "aws-smithy-legacy-http-server")
))]
pub(crate) use aws_smithy_http_server_065 as aws_smithy_http_server;

/// For post-http1x codegen version support, if http1x is not used
/// Takes precedence if both "aws-smithy-http-server-065" and "aws-smithy-legacy-http-server" are enabled
#[cfg(feature = "aws-smithy-legacy-http-server")]
pub(crate) use aws_smithy_legacy_http_server as aws_smithy_http_server;
#[cfg(any(
    feature = "aws-smithy-http-server-065",
    feature = "aws-smithy-legacy-http-server"
))]
use http_02 as http;
#[cfg(any(
    feature = "aws-smithy-http-server-065",
    feature = "aws-smithy-legacy-http-server"
))]
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
    #[cfg(not(any(
        feature = "aws-smithy-http-server-065",
        feature = "aws-smithy-legacy-http-server"
    )))]
    return aws_smithy_http_server::body::empty();
    #[cfg(any(
        feature = "aws-smithy-http-server-065",
        feature = "aws-smithy-legacy-http-server"
    ))]
    return aws_smithy_http_server::body::boxed(http_body_04::Empty::new());
}
