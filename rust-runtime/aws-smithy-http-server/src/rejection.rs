/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Rejection types.
//!
//! This module contains types that are commonly used as the `E` error type in functions that
//! handle requests and responses that return `Result<T, E>` throughout the framework. These
//! include functions to deserialize incoming requests and serialize outgoing responses.
//!
//! There are three main types:
//!
//! 1. [`RequestRejection`]s are used when the framework fails to deserialize the request into the
//!    corresponding operation input.
//! 1. [`ExtensionHandlingRejection`]s are used when the framework fails to deserialize the request
//!    into a [`super::Extension`] registered by the service.
//! 1. [`ResponseRejection`]s are used when the framework fails to serialize the operation
//!    output into a response.
//!
//! They are called _rejection_ types and not _error_ types to signal that the input was _rejected_
//! (as opposed to it causing a recoverable error that would need to be handled, or an
//! unrecoverable error). For example, a [`RequestRejection`] simply means that the request was
//! rejected; there isn't really anything wrong with the service itself that the service
//! implementer would need to handle.
//!
//! Rejection types are an _internal_ detail about the framework, they can be added, removed, and
//! modified at any time without causing breaking changes. They are not surfaced to clients or the
//! service implementer in any way (including this documentation): indeed, they can't be converted
//! into responses. They serve as a mechanism to keep track of all the possible errors that can
//! occur when processing a request or a response, in far more detail than what AWS protocols need
//! to. This is why they are so granular: other (possibly protocol-specific) error types (like
//! [`crate::exception::SmithyFrameworkException`]) can "group" them when exposing errors to
//! clients while the framework does not need to sacrifice fidelity in private error handling
//! routines, and future-proofing itself at the same time (for example, we might want to record
//! metrics about rejection types).
//!
//! Rejection types implement [`std::error::Error`], and some take in type-erased boxed errors
//! ([`crate::Error`]) to represent their underlying causes, so they can be composed with other
//! types that take in (possibly type-erased) [`std::error::Error`]s, like
//! `crate::exception::SmithyFrameworkExceptionType`, thus allowing us to represent the full error
//! chain.

use strum_macros::Display;

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

/// Errors that can occur when serializing the operation output provided by the service implementer
/// into an HTTP response.
#[derive(Debug, Display)]
pub enum ResponseRejection {
    /// Used when `httpResponseCode` targets an optional member, and the service implementer sets
    /// it to `None`.
    MissingHttpStatusCode,

    /// Used when the service implementer provides an integer outside the 100-999 range for a
    /// member targeted by `httpResponseCode`.
    InvalidHttpStatusCode,

    /// Used when an invalid HTTP header value (a value that cannot be parsed as an
    /// `[http::header::HeaderValue]`) is provided for a shape member bound to an HTTP header with
    /// `httpHeader` or `httpPrefixHeaders`.
    /// Used when failing to serialize an `httpPayload`-bound struct into an HTTP response body.
    Build(crate::Error),

    /// Used when failing to serialize a struct into a `String` for the HTTP response body (for
    /// example, converting a struct into a JSON-encoded `String`).
    Serialization(crate::Error),

    /// Used when consuming an [`http::Response::Builder`] into the constructed [`http::Response`]
    /// when calling [`http::Response::builder::body`].
    /// This error can happen if an invalid HTTP header value (a value that cannot be parsed as an
    /// `[http::header::HeaderValue]`) is used for the protocol-specific response `Content-Type`
    /// header, or for additional protocol-specific headers (like `X-Amzn-Errortype` to signal
    /// errors in RestJson1).
    Http(crate::Error),
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

/// Errors that can occur when deserializing an HTTP request into an _operation input_, the input
/// that is passed as the first argument to operation handlers. To deserialize into the service's
/// registered state, a different rejection type is used, [`self::ExtensionHandlingRejection`].
///
/// This type allows us to easily keep track of all the possible errors that can occur in the
/// lifecycle of an incoming HTTP request.
///
/// Many inner code-generated and runtime deserialization functions use this as their error type, when they can
/// only instantiate a subset of the variants (most likely a single one). For example, the
/// functions that check the `Content-Type` header in `[crate::protocols]` can only return three of
/// the variants: `MissingJsonContentType`, `MissingXmlContentType`, and `MimeParse`.
/// This is a deliberate design choice to keep code generation simple. After all, this type is an
/// inner detail of the framework the service implementer does not interact with. It allows us to
/// easily keep track of all the possible errors that can occur in the lifecycle of an incoming
/// HTTP request.
///
/// If a variant takes in a value, it represents the underlying cause of the error. This inner
/// value should be of the type-erased boxed error type `[crate::Error]`. In practice, some of the
/// variants that take in a value are only instantiated with errors of a single type in the
/// generated code. For example, `UriPatternMismatch` is only instantiated with an error coming
/// from a `nom` parser, `nom::Err<nom::error::Error<&str>>`. This is reflected in the converters
/// below that convert from one of these very specific error types into one of the variants. For
/// example, the `RequestRejection` implements `From<hyper::Error>` to construct the `HttpBody`
/// variant. This is a deliberate design choice to make the code simpler and less prone to changes.
///
// The variants are _roughly_ sorted in the order in which the HTTP request is processed.
#[derive(Debug, Display)]
pub enum RequestRejection {
    /// Used when attempting to take the request's body, and it has already been taken (presumably
    /// by an outer `Service` that handled the request before us).
    BodyAlreadyExtracted,

    // Used when failing to convert non-streaming requests into a byte slab with
    // `hyper::body::to_bytes`.
    HttpBody(crate::Error),

    /// These are used when checking the `Content-Type` header.
    MissingJsonContentType,
    MissingXmlContentType,
    MimeParse,

    /// Used when failing to deserialize the HTTP body's bytes into a JSON document conforming to
    /// the modeled input it should represent.
    JsonDeserialize(crate::Error),
    /// Used when failing to deserialize the HTTP body's bytes into a XML conforming to the modeled
    /// input it should represent.
    XmlDeserialize(crate::Error),

    /// Used when attempting to take the request's headers, and they have already been taken (presumably
    /// by an outer `Service` that handled the request before us).
    HeadersAlreadyExtracted,

    /// Used when failing to parse HTTP headers that are bound to input members with the `httpHeader`
    /// or the `httpPrefixHeaders` traits.
    HeaderParse(crate::Error),

    /// Used when the URI pattern has a literal after the greedy label, and it is not found in the
    /// request's URL.
    UriPatternGreedyLabelPostfixNotFound,
    /// Used when the `nom` parser's input does not match the URI pattern.
    UriPatternMismatch(crate::Error),

    /// Used when percent-decoding URL query string.
    /// Used when percent-decoding URI path label.
    InvalidUtf8(crate::Error),

    /// Used when failing to deserialize strings from a URL query string and from URI path labels
    /// into an [`aws_smithy_types::DateTime`].
    DateTimeParse(crate::Error),

    /// Used when failing to deserialize strings from a URL query string and from URI path labels
    /// into "primitive" types.
    PrimitiveParse(crate::Error),

    /// The following three variants are used when failing to deserialize strings from a URL query
    /// string and URI path labels into "primitive" types.
    /// TODO(https://github.com/awslabs/smithy-rs/issues/1232): They should be removed and
    /// conflated into the `PrimitiveParse` variant above after this issue is resolved.
    IntParse(crate::Error),
    FloatParse(crate::Error),
    BoolParse(crate::Error),

    /// Used when consuming the input struct builder.
    /// TODO Can we make builders non-fallible in the server? Or just don't use them at all in
    /// request deserialization?
    Build(crate::Error),
}

impl std::error::Error for RequestRejection {}

/// These converters are solely to make code-generation simpler. They convert from a specific error
/// type (from a runtime/third-party crate or the standard library) into a variant of the
/// [`crate::rejection::RequestRejection`] enum holding the type-erased boxed [`crate::Error`]
/// type. Generated functions that use [crate::rejection::RequestRejection] can thus use `?` to
/// bubble up instead of having to sprinkle things like [`Result::map_err`] everywhere.

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
    fn from(err: nom::Err<nom::error::Error<&str>>) -> Self {
        Self::UriPatternMismatch(crate::Error::new(err.to_owned()))
    }
}

/// Used when calling [`percent_encoding::percent_decode_str`](https://docs.rs/percent-encoding/latest/percent_encoding/fn.percent_decode_str.html) and bubbling up.
/// TODO(https://github.com/servo/rust-url/issues/758): Unless I'm missing something, percent-decoding can't fail, so we could just `.expect()`.
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

/// `[crate::body::Body]` is `[hyper::Body]`, whose associated `Error` type is `[hyper::Error]`. We
/// need this converter for when we convert the body into bytes in the framework, since protocol
/// tests use `[crate::body::Body]` as their body type when constructing requests (and almost
/// everyone will run a Hyper-based server in their services).
impl From<hyper::Error> for RequestRejection {
    fn from(err: hyper::Error) -> Self {
        Self::HttpBody(crate::Error::new(err))
    }
}
