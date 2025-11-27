/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Rejection types.
//!
//! This module contains types that are commonly used as the `E` error type in functions that
//! handle requests and responses that return `Result<T, E>` throughout the framework. These
//! include functions to deserialize incoming requests and serialize outgoing responses.
//!
//! All types end with `Rejection`. There are two types:
//!
//! 1. [`RequestRejection`]s are used when the framework fails to deserialize the request into the
//!    corresponding operation input.
//! 2. [`ResponseRejection`]s are used when the framework fails to serialize the operation
//!    output into a response.
//!
//! They are called _rejection_ types and not _error_ types to signal that the input was _rejected_
//! (as opposed to it causing a recoverable error that would need to be handled, or an
//! unrecoverable error). For example, a [`RequestRejection`] simply means that the request was
//! rejected; there isn't really anything wrong with the service itself that the service
//! implementer would need to handle.
//!
//! Rejection types are an _internal_ detail about the framework: they can be added, removed, and
//! modified at any time without causing breaking changes. They are not surfaced to clients or the
//! service implementer in any way (including this documentation): indeed, they can't be converted
//! into responses. They serve as a mechanism to keep track of all the possible errors that can
//! occur when processing a request or a response, in far more detail than what AWS protocols need
//! to. This is why they are so granular: other (possibly protocol-specific) error types (like
//! [`crate::protocol::rest_json_1::runtime_error::RuntimeError`]) can "group" them when exposing
//! errors to clients while the framework does not need to sacrifice fidelity in private error
//! handling routines, and future-proofing itself at the same time (for example, we might want to
//! record metrics about rejection types).
//!
//! Rejection types implement [`std::error::Error`], and some take in type-erased boxed errors
//! (`crate::Error`) to represent their underlying causes, so they can be composed with other types
//! that take in (possibly type-erased) [`std::error::Error`]s, like
//! [`crate::protocol::rest_json_1::runtime_error::RuntimeError`], thus allowing us to represent the
//! full error chain.
//!
//! This module hosts rejection types _specific_ to the [`crate::protocol::rest_json_1`] protocol, but
//! the paragraphs above apply to _all_ protocol-specific rejection types.
//!
//! Similarly, rejection type variants are exhaustively documented solely in this module if they have
//! direct counterparts in other protocols. This is to avoid documentation getting out of date.
//!
//! Consult `crate::protocol::$protocolName::rejection` for rejection types for other protocols.

use crate::rejection::MissingContentTypeReason;
use aws_smithy_runtime_api::http::HttpError;
use std::num::TryFromIntError;
use thiserror::Error;

/// Errors that can occur when serializing the operation output provided by the service implementer
/// into an HTTP response.
#[derive(Debug, Error)]
pub enum ResponseRejection {
    /// Used when the service implementer provides an integer outside the 100-999 range for a
    /// member targeted by `httpResponseCode`.
    /// See <https://github.com/awslabs/smithy/issues/1116>.
    #[error("invalid bound HTTP status code; status codes must be inside the 100-999 range: {0}")]
    InvalidHttpStatusCode(TryFromIntError),

    /// Used when an invalid HTTP header name (a value that cannot be parsed as an
    /// [`http::header::HeaderName`]) or HTTP header value (a value that cannot be parsed as an
    /// [`http::header::HeaderValue`]) is provided for a shape member bound to an HTTP header with
    /// `httpHeader` or `httpPrefixHeaders`.
    /// Used when failing to serialize an `httpPayload`-bound struct into an HTTP response body.
    #[error("error building HTTP response: {0}")]
    Build(#[from] aws_smithy_types::error::operation::BuildError),

    /// Used when failing to serialize a struct into a `String` for the JSON-encoded HTTP response
    /// body.
    /// Fun fact: as of writing, this can only happen when date formatting
    /// (`aws_smithy_types::date_time::DateTime:fmt`) fails, which can only happen if the
    /// supplied timestamp is outside of the valid range when formatting using RFC-3339, i.e. a
    /// date outside the `0001-01-01T00:00:00.000Z`-`9999-12-31T23:59:59.999Z` range is supplied.
    #[error("error serializing JSON-encoded body: {0}")]
    Serialization(#[from] aws_smithy_types::error::operation::SerializationError),

    /// Used when consuming an [`http::response::Builder`] into the constructed [`http::Response`]
    /// when calling [`http::response::Builder::body`].
    /// This error can happen if an invalid HTTP header value (a value that cannot be parsed as an
    /// `[http::header::HeaderValue]`) is used for the protocol-specific response `Content-Type`
    /// header, or for additional protocol-specific headers (like `X-Amzn-Errortype` to signal
    /// errors in RestJson1).
    #[error("error building HTTP response: {0}")]
    HttpBuild(#[from] http::Error),
}

/// Errors that can occur when deserializing an HTTP request into an _operation input_, the input
/// that is passed as the first argument to operation handlers.
///
/// This type allows us to easily keep track of all the possible errors that can occur in the
/// lifecycle of an incoming HTTP request.
///
/// Many inner code-generated and runtime deserialization functions use this as their error type,
/// when they can only instantiate a subset of the variants (most likely a single one). This is a
/// deliberate design choice to keep code generation simple. After all, this type is an inner
/// detail of the framework the service implementer does not interact with.
///
/// If a variant takes in a value, it represents the underlying cause of the error.
///
/// The variants are _roughly_ sorted in the order in which the HTTP request is processed.
#[derive(Debug, Error)]
pub enum RequestRejection {
    /// Used when failing to convert non-streaming requests into a byte slab with
    /// `hyper::body::to_bytes`.
    #[error("error converting non-streaming body to bytes: {0}")]
    BufferHttpBodyBytes(crate::Error),

    /// Used when the request contained an `Accept` header with a MIME type, and the server cannot
    /// return a response body adhering to that MIME type.
    #[error("request contains invalid value for `Accept` header")]
    NotAcceptable,

    /// Used when checking the `Content-Type` header.
    /// This is bubbled up in the generated SDK when calling
    /// [`crate::protocol::content_type_header_classifier_smithy`] in `from_request`.
    #[error("expected `Content-Type` header not found: {0}")]
    MissingContentType(#[from] MissingContentTypeReason),

    /// Used when failing to deserialize the HTTP body's bytes into a JSON document conforming to
    /// the modeled input it should represent.
    #[error("error deserializing request HTTP body as JSON: {0}")]
    JsonDeserialize(#[from] aws_smithy_json::deserialize::error::DeserializeError),

    /// Used when failing to parse HTTP headers that are bound to input members with the `httpHeader`
    /// or the `httpPrefixHeaders` traits.
    #[error("error binding request HTTP headers: {0}")]
    HeaderParse(#[from] aws_smithy_http::header::ParseError),

    // In theory, the next two errors should never happen because the router should have already
    // rejected the request.
    /// Used when the URI pattern has a literal after the greedy label, and it is not found in the
    /// request's URL.
    #[error("request URI does not match pattern because of literal suffix after greedy label was not found")]
    UriPatternGreedyLabelPostfixNotFound,
    /// Used when the `nom` parser's input does not match the URI pattern.
    #[error("request URI does not match `@http` URI pattern: {0}")]
    UriPatternMismatch(crate::Error),

    /// Used when percent-decoding URL query string.
    /// Used when percent-decoding URI path label.
    /// This is caused when calling
    /// [`percent_encoding::percent_decode_str`](https://docs.rs/percent-encoding/latest/percent_encoding/fn.percent_decode_str.html).
    /// This can happen when the percent-encoded data decodes to bytes that are
    /// not a well-formed UTF-8 string.
    #[error("request URI cannot be percent decoded into valid UTF-8")]
    PercentEncodedUriNotValidUtf8(#[from] core::str::Utf8Error),

    /// Used when failing to deserialize strings from a URL query string and from URI path labels
    /// into an [`aws_smithy_types::DateTime`].
    #[error("error parsing timestamp from request URI: {0}")]
    DateTimeParse(#[from] aws_smithy_types::date_time::DateTimeParseError),

    /// Used when failing to deserialize strings from a URL query string and from URI path labels
    /// into "primitive" types.
    #[error("error parsing primitive type from request URI: {0}")]
    PrimitiveParse(#[from] aws_smithy_types::primitive::PrimitiveParseError),

    /// Used when consuming the input struct builder, and constraint violations occur.
    // This rejection is constructed directly in the code-generated SDK instead of in this crate.
    #[error("request does not adhere to modeled constraints: {0}")]
    ConstraintViolation(String),

    /// Typically happens when the request has headers that are not valid UTF-8.
    #[error("failed to convert request: {0}")]
    HttpConversion(#[from] HttpError),
}

// Consider a conversion between `T` and `U` followed by a bubbling up of the conversion error
// through `Result<_, RequestRejection>`. This [`From`] implementation accomodates the special case
// where `T` and `U` are equal, in such cases `T`/`U` a enjoy `TryFrom<T>` with
// `Err = std::convert::Infallible`.
//
// Note that when `!` stabilizes `std::convert::Infallible` will become an alias for `!` and there
// will be a blanket `impl From<!> for T`. This will remove the need for this implementation.
//
// More details on this can be found in the following links:
// - https://doc.rust-lang.org/std/primitive.never.html
// - https://doc.rust-lang.org/std/convert/enum.Infallible.html#future-compatibility
impl From<std::convert::Infallible> for RequestRejection {
    fn from(_err: std::convert::Infallible) -> Self {
        // We opt for this `match` here rather than [`unreachable`] to assure the reader that this
        // code path is dead.
        match _err {}
    }
}

// Conversion from crate::Error is needed for custom body types and testing scenarios.
// When using BoxBody or custom body implementations, errors are crate::Error, not hyper::Error.
impl From<crate::Error> for RequestRejection {
    fn from(err: crate::Error) -> Self {
        Self::BufferHttpBodyBytes(err)
    }
}

// These converters are solely to make code-generation simpler. They convert from a specific error
// type (from a runtime/third-party crate or the standard library) into a variant of the
// [`crate::rejection::RequestRejection`] enum holding the type-erased boxed [`crate::Error`]
// type. Generated functions that use [crate::rejection::RequestRejection] can thus use `?` to
// bubble up instead of having to sprinkle things like [`Result::map_err`] everywhere.

impl From<nom::Err<nom::error::Error<&str>>> for RequestRejection {
    fn from(err: nom::Err<nom::error::Error<&str>>) -> Self {
        Self::UriPatternMismatch(crate::Error::new(err.to_owned()))
    }
}

// Hyper's HTTP server provides requests with `hyper::body::Incoming`, which has error type
// `hyper::Error`. During request deserialization (FromRequest), body operations can produce
// this error, so we need this conversion to handle it within the framework.
convert_to_request_rejection!(hyper::Error, BufferHttpBodyBytes);

// Useful in general, but it also required in order to accept Lambda HTTP requests using
// `Router<lambda_http::Body>` since `lambda_http::Error` is a type alias for `Box<dyn Error + ..>`.
convert_to_request_rejection!(Box<dyn std::error::Error + Send + Sync + 'static>, BufferHttpBodyBytes);
