/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Rejection response types.

define_rejection! {
    #[status = INTERNAL_SERVER_ERROR]
    #[body = "Extensions taken by other extractor"]
    /// Rejection used if the request extension has been taken by another
    /// extractor.
    pub struct ExtensionsAlreadyExtracted;
}

define_rejection! {
    #[status = INTERNAL_SERVER_ERROR]
    #[body = "Missing request extension"]
    /// Rejection type for [`Extension`](super::Extension) if an expected
    /// request extension was not found.
    pub struct MissingExtension(Error);
}

composite_rejection! {
    /// Rejection used for [`Extension`](super::Extension).
    ///
    /// Contains one variant for each way the [`Extension`](super::Extension) extractor
    /// can fail.
    pub enum ExtensionHandlingRejection {
        MissingExtension,
        ExtensionsAlreadyExtracted,
    }
}

/// These are errors that can occur when transforming the operation output into an HTTP response.
#[derive(Debug)]
pub enum ResponseRejection {
    /// Used when adding HTTP headers (e.g. a value cannot be used as a `HeaderValue`).
    Build(crate::Error),
    /// Used when serializing struct into HTTP response bodies.
    Serialization(crate::Error),
    /// Used when converting the HTTP response builder into an HTTP response.
    /// TODO I think this could be removed if we didn't use HTTP response builder and instead used
    /// `*_mut` methods.
    Http(crate::Error),
}

impl std::fmt::Display for ResponseRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO Fill out.
        // TODO Consider deriving it with `enum_display_derive` or `thiserror`.
        write!(f, "{}", self)
    }
}

impl std::error::Error for ResponseRejection {}

impl From<aws_smithy_http::operation::BuildError> for ResponseRejection {
    fn from(err: aws_smithy_http::operation::BuildError) -> Self {
        Self::Build(crate::Error::new(err))
    }
}

impl From<aws_smithy_http::operation::SerializationError> for ResponseRejection {
    fn from(err: aws_smithy_http::operation::SerializationError) -> Self {
        Self::Serialization(crate::Error::new(err))
    }
}

impl From<http::Error> for ResponseRejection {
    fn from(err: http::Error) -> Self {
        Self::Http(crate::Error::new(err))
    }
}

// Deserialization functions return this as error.
// TODO Document precisely when all of these are created.
// TODO Sort them by the order in which you would encounter them.
// Note these are rejections that occur when constructing first argument. For state, there is the
// `ExtensionHandlingRejection`.
#[derive(Debug)]
pub enum RequestRejection {
    /// Used when percent decoding query string.
    InvalidUtf8(crate::Error),
    JsonDeserialize(crate::Error),
    XmlDeserialize(crate::Error),
    BodyAlreadyExtracted,
    HeadersAlreadyExtracted,
    /// Used when parsing HTTP headers that are bound to input members (httpHeader,
    /// httpPrefixHeaders).
    HeaderParse(crate::Error),

    DateTimeParse(crate::Error),

    /// TODO This one should replace the below 3 when we refactor parsing code to use aws_smithy_types
    /// parse.
    PrimitiveParse(crate::Error),

    /// Maybe these 3 is too much detail. Maybe not.
    IntParse(crate::Error),
    FloatParse(crate::Error),
    BoolParse(crate::Error),

    /// When the nom parser's input does not match the URI pattern.
    UriPatternMismatch(crate::Error),
    /// This is also returned when the URI pattern has a suffix after the greedy label, and we
    /// special-case that check in the generated code.
    UriPatternGreedyLabelPostfixNotFound,

    Build(crate::Error),
    // TODO when hyper.to_bytes() fails
    HttpBody(crate::Error),
    // TODO We need to handwrite `ContentTypeRejection`.

    // Related to checking the `Content-Type` header.
    MissingJsonContentType,
    MissingXmlContentType,
    MimeParse,
}

impl std::fmt::Display for RequestRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO Fill out.
        // TODO Consider deriving it with `enum_display_derive` or `thiserror`.
        write!(f, "{}", self)
    }
}

impl std::error::Error for RequestRejection {}

// TODO These could be generated with a macro

impl From<aws_smithy_json::deserialize::Error> for RequestRejection {
    fn from(err: aws_smithy_json::deserialize::Error) -> Self {
        Self::JsonDeserialize(crate::Error::new(err))
    }
}

impl From<aws_smithy_xml::decode::XmlError> for RequestRejection {
    fn from(err: aws_smithy_xml::decode::XmlError) -> Self {
        Self::XmlDeserialize(crate::Error::new(err))
    }
}

impl From<aws_smithy_http::operation::BuildError> for RequestRejection {
    fn from(err: aws_smithy_http::operation::BuildError) -> Self {
        Self::Build(crate::Error::new(err))
    }
}

impl From<aws_smithy_http::header::ParseError> for RequestRejection {
    fn from(err: aws_smithy_http::header::ParseError) -> Self {
        Self::HeaderParse(crate::Error::new(err))
    }
}

impl From<aws_smithy_types::date_time::DateTimeParseError> for RequestRejection {
    fn from(err: aws_smithy_types::date_time::DateTimeParseError) -> Self {
        Self::DateTimeParse(crate::Error::new(err))
    }
}

impl From<aws_smithy_types::primitive::PrimitiveParseError> for RequestRejection {
    fn from(err: aws_smithy_types::primitive::PrimitiveParseError) -> Self {
        Self::PrimitiveParse(crate::Error::new(err))
    }
}

impl From<std::str::ParseBoolError> for RequestRejection {
    fn from(err: std::str::ParseBoolError) -> Self {
        Self::BoolParse(crate::Error::new(err))
    }
}

impl From<std::num::ParseFloatError> for RequestRejection {
    fn from(err: std::num::ParseFloatError) -> Self {
        Self::FloatParse(crate::Error::new(err))
    }
}

impl From<std::num::ParseIntError> for RequestRejection {
    fn from(err: std::num::ParseIntError) -> Self {
        Self::IntParse(crate::Error::new(err))
    }
}

impl From<nom::Err<nom::error::Error<&str>>> for RequestRejection {
    // TODO Can't we make the parser return `Error<String>`?
    fn from(err: nom::Err<nom::error::Error<&str>>) -> Self {
        Self::UriPatternMismatch(crate::Error::new(err.to_owned()))
    }
}

// TODO I'm not sure we need this one. Because serde_urlencoded would have already failed if it's
// not UTF-8.
// i.e. do we really need `percent_encoding::percent_decode_str(value)?
impl From<std::str::Utf8Error> for RequestRejection {
    fn from(err: std::str::Utf8Error) -> Self {
        Self::InvalidUtf8(crate::Error::new(err))
    }
}

impl From<serde_urlencoded::de::Error> for RequestRejection {
    fn from(err: serde_urlencoded::de::Error) -> Self {
        Self::InvalidUtf8(crate::Error::new(err))
    }
}

// impl From<http::Error> for FromRequest {
//     fn from(err: http::Error) -> Self {
//         Self::HttpBody(crate::Error::new(err))
//     }
// }

/// `[crate::body::Body]` is `[hyper::Body]`, whose associated `Error` type is `[hyper::Error]`. We
/// need this converter for when we convert the body into bytes in the framework, since protocol
/// tests use `[crate::body::Body]` as their body type when constructing requests.
impl From<hyper::Error> for RequestRejection {
    fn from(err: hyper::Error) -> Self {
        Self::HttpBody(crate::Error::new(err))
    }
}
