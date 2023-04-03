/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! This module hosts _exactly_ the same as [`crate::proto::rest_json_1::rejection`], expect that
//! [`crate::proto::rest_json_1::rejection::RequestRejection::JsonDeserialize`] is swapped for
//! [`RequestRejection::XmlDeserialize`].

use strum_macros::Display;

#[derive(Debug, Display)]
pub enum ResponseRejection {
    InvalidHttpStatusCode,
    Build(crate::Error),
    Serialization(crate::Error),
    Http(crate::Error),
}

impl std::error::Error for ResponseRejection {}

convert_to_response_rejection!(aws_smithy_http::operation::error::BuildError, Build);
convert_to_response_rejection!(aws_smithy_http::operation::error::SerializationError, Serialization);
convert_to_response_rejection!(http::Error, Http);

#[derive(Debug, Display)]
pub enum RequestRejection {
    HttpBody(crate::Error),

    MissingContentType(MissingContentTypeReason),

    /// Used when failing to deserialize the HTTP body's bytes into a XML conforming to the modeled
    /// input it should represent.
    XmlDeserialize(crate::Error),

    HeaderParse(crate::Error),

    UriPatternGreedyLabelPostfixNotFound,
    UriPatternMismatch(crate::Error),

    InvalidUtf8(crate::Error),

    DateTimeParse(crate::Error),

    PrimitiveParse(crate::Error),

    ConstraintViolation(String),
}

#[derive(Debug, Display)]
pub enum MissingContentTypeReason {
    HeadersTakenByAnotherExtractor,
    NoContentTypeHeader,
    ToStrError(http::header::ToStrError),
    MimeParseError(mime::FromStrError),
    UnexpectedMimeType {
        expected_mime: Option<mime::Mime>,
        found_mime: Option<mime::Mime>,
    },
}

impl std::error::Error for RequestRejection {}

impl From<std::convert::Infallible> for RequestRejection {
    fn from(_err: std::convert::Infallible) -> Self {
        match _err {}
    }
}

impl From<MissingContentTypeReason> for RequestRejection {
    fn from(e: MissingContentTypeReason) -> Self {
        Self::MissingContentType(e)
    }
}

convert_to_request_rejection!(aws_smithy_xml::decode::XmlDecodeError, XmlDeserialize);
convert_to_request_rejection!(aws_smithy_http::header::ParseError, HeaderParse);
convert_to_request_rejection!(aws_smithy_types::date_time::DateTimeParseError, DateTimeParse);
convert_to_request_rejection!(aws_smithy_types::primitive::PrimitiveParseError, PrimitiveParse);
convert_to_request_rejection!(serde_urlencoded::de::Error, InvalidUtf8);

impl From<nom::Err<nom::error::Error<&str>>> for RequestRejection {
    fn from(err: nom::Err<nom::error::Error<&str>>) -> Self {
        Self::UriPatternMismatch(crate::Error::new(err.to_owned()))
    }
}

convert_to_request_rejection!(std::str::Utf8Error, InvalidUtf8);

convert_to_request_rejection!(hyper::Error, HttpBody);

convert_to_request_rejection!(Box<dyn std::error::Error + Send + Sync + 'static>, HttpBody);
