/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! This module hosts _exactly_ the same as [`crate::protocol::rest_json_1::rejection`], except that
//! [`crate::protocol::rest_json_1::rejection::RequestRejection::JsonDeserialize`] is swapped for
//! [`RequestRejection::XmlDeserialize`].

use crate::rejection::MissingContentTypeReason;
use aws_smithy_runtime_api::http::HttpError;
use std::num::TryFromIntError;
use thiserror::Error;

use crate::http;

#[derive(Debug, Error)]
pub enum ResponseRejection {
    #[error("invalid bound HTTP status code; status codes must be inside the 100-999 range: {0}")]
    InvalidHttpStatusCode(TryFromIntError),
    #[error("error building HTTP response: {0}")]
    Build(#[from] aws_smithy_types::error::operation::BuildError),
    #[error("error serializing XML-encoded body: {0}")]
    Serialization(#[from] aws_smithy_types::error::operation::SerializationError),
    #[error("error building HTTP response: {0}")]
    HttpBuild(#[from] http::Error),
}

#[derive(Debug, Error)]
pub enum RequestRejection {
    #[error("error converting non-streaming body to bytes: {0}")]
    BufferHttpBodyBytes(crate::Error),

    #[error("request contains invalid value for `Accept` header")]
    NotAcceptable,

    #[error("expected `Content-Type` header not found: {0}")]
    MissingContentType(#[from] MissingContentTypeReason),

    /// Used when failing to deserialize the HTTP body's bytes into a XML conforming to the modeled
    /// input it should represent.
    #[error("error deserializing request HTTP body as XML: {0}")]
    XmlDeserialize(#[from] aws_smithy_xml::decode::XmlDecodeError),

    #[error("error binding request HTTP headers: {0}")]
    HeaderParse(#[from] aws_smithy_http::header::ParseError),

    #[error("request URI does not match pattern because of literal suffix after greedy label was not found")]
    UriPatternGreedyLabelPostfixNotFound,
    #[error("request URI does not match `@http` URI pattern: {0}")]
    UriPatternMismatch(crate::Error),

    #[error("request URI cannot be percent decoded into valid UTF-8")]
    PercentEncodedUriNotValidUtf8(#[from] core::str::Utf8Error),

    #[error("error parsing timestamp from request URI: {0}")]
    DateTimeParse(#[from] aws_smithy_types::date_time::DateTimeParseError),

    #[error("error parsing primitive type from request URI: {0}")]
    PrimitiveParse(#[from] aws_smithy_types::primitive::PrimitiveParseError),

    #[error("request does not adhere to modeled constraints: {0}")]
    ConstraintViolation(String),

    /// Typically happens when the request has headers that are not valid UTF-8.
    #[error("failed to convert request: {0}")]
    HttpConversion(#[from] HttpError),
}

impl From<std::convert::Infallible> for RequestRejection {
    fn from(_err: std::convert::Infallible) -> Self {
        match _err {}
    }
}

// Enable conversion from crate::Error for general body buffering error handling
impl From<crate::Error> for RequestRejection {
    fn from(err: crate::Error) -> Self {
        Self::BufferHttpBodyBytes(err)
    }
}

impl From<nom::Err<nom::error::Error<&str>>> for RequestRejection {
    fn from(err: nom::Err<nom::error::Error<&str>>) -> Self {
        Self::UriPatternMismatch(crate::Error::new(err.to_owned()))
    }
}

convert_to_request_rejection!(hyper::Error, BufferHttpBodyBytes);

convert_to_request_rejection!(Box<dyn std::error::Error + Send + Sync + 'static>, BufferHttpBodyBytes);
