/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime error type.
//!
//! This module contains the [`RuntimeError`] type.
//!
//! As opposed to rejection types (see [`crate::protocol::rest_json_1::rejection`]), which are an internal detail about
//! the framework, `RuntimeError` is surfaced to clients in HTTP responses: indeed, it implements
//! [`RuntimeError::into_response`]. Rejections can be "grouped" and converted into a
//! specific `RuntimeError` kind: for example, all request rejections due to serialization issues
//! can be conflated under the [`RuntimeError::Serialization`] enum variant.
//!
//! The HTTP response representation of the specific `RuntimeError` is protocol-specific: for
//! example, the runtime error in the [`crate::protocol::rest_json_1`] protocol sets the `X-Amzn-Errortype` header.
//!
//! Generated code works always works with [`crate::rejection`] types when deserializing requests
//! and serializing response. Just before a response needs to be sent, the generated code looks up
//! and converts into the corresponding `RuntimeError`, and then it uses the its
//! [`RuntimeError::into_response`] method to render and send a response.
//!
//! This module hosts the `RuntimeError` type _specific_ to the [`crate::protocol::rest_json_1`] protocol, but
//! the paragraphs above apply to _all_ protocol-specific rejection types.
//!
//! Similarly, `RuntimeError` variants are exhaustively documented solely in this module if they have
//! direct counterparts in other protocols. This is to avoid documentation getting out of date.
//!
//! Consult `crate::protocol::$protocolName::runtime_error` for the `RuntimeError` type for other protocols.

use super::rejection::RequestRejection;
use super::rejection::ResponseRejection;
use super::RestJson1;
use crate::extension::RuntimeErrorExtension;
use crate::modeled_error::HttpModeledError;
use crate::response::IntoResponse;
use crate::runtime_error::InternalFailureException;
use crate::runtime_error::INVALID_HTTP_RESPONSE_FOR_RUNTIME_ERROR_PANIC_MESSAGE;
use crate::schema::protocol::ServerProtocol;
use http::StatusCode;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// Request failed to deserialize or response failed to serialize.
    #[error("request failed to deserialize or response failed to serialize: {0}")]
    Serialization(crate::Error),
    /// As of writing, this variant can only occur upon failure to extract an
    /// [`crate::extension::Extension`] from the request.
    #[error("internal failure: {0}")]
    InternalFailure(crate::Error),
    /// Request contains an `Accept` header with a MIME type, and the server cannot return a response
    /// body adhering to that MIME type.
    // This is returned directly (i.e. without going through a [`RequestRejection`] first) in the
    // generated SDK when calling [`crate::protocol::accept_header_classifier`] in
    // `from_request`.
    #[error("not acceptable request: request contains an `Accept` header with a MIME type, and the server cannot return a response body adhering to that MIME type")]
    NotAcceptable,
    /// The request does not contain the expected `Content-Type` header value.
    #[error("unsupported media type: request does not contain the expected `Content-Type` header value")]
    UnsupportedMediaType,
    /// Operation input contains data that does not adhere to the modeled [constraint traits].
    /// Carries the modeled validation error, serialized once at the protocol
    /// boundary via `ServerProtocol::serialize_error`.
    /// [constraint traits]: <https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html>
    #[error("validation failure: operation input contains data that does not adhere to the modeled constraints: {0}")]
    Validation(String),
    /// Schema-driven validation failure carrying the actual modeled validation
    /// error. Serialized once by the protocol codec.
    #[error("validation failure: operation input contains data that does not adhere to the modeled constraints: {0}")]
    ModeledValidation(Box<dyn HttpModeledError + Send>),
}

impl RuntimeError {
    /// String representation of the `RuntimeError` kind.
    /// Used as the value passed to construct an [`crate::extension::RuntimeErrorExtension`].
    /// Used as the value of the `X-Amzn-Errortype` header.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Serialization(_) => "SerializationException",
            Self::InternalFailure(_) => "InternalFailureException",
            Self::NotAcceptable => "NotAcceptableException",
            Self::UnsupportedMediaType => "UnsupportedMediaTypeException",
            Self::Validation(_) => "ValidationException",
            Self::ModeledValidation(_) => "ValidationException",
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Serialization(_) => StatusCode::BAD_REQUEST,
            Self::InternalFailure(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::NotAcceptable => StatusCode::NOT_ACCEPTABLE,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::ModeledValidation(err) => StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::BAD_REQUEST),
        }
    }
}

impl IntoResponse<RestJson1> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        IntoResponse::<RestJson1>::into_response(RuntimeError::InternalFailure(crate::Error::new(String::new())))
    }
}

// Why is only `ModeledValidation` schema-driven, while the other variants keep
// hand-assembled responses?
//
// `ModeledValidation` carries an actual modeled shape
// (`smithy.framework#ValidationException` or a decorator-customized shape) —
// it has a schema, so `serialize_error` applies. The legacy `Validation`
// variant still carries protocol-specific pre-schema artifacts and remains on
// the old hand-assembled response path. The remaining variants are framework
// conventions with NO Smithy shape behind them, and they deliberately stay
// hand-assembled this phase:
//
// - Their frozen legacy wire forms are not the serialization of *any* shape:
//   restJson1 sends a literal `{}`, awsJson1.1 an empty body, rpcv2Cbor an
//   empty map with no `__type` (#3716), restXml the string `{}` on an XML
//   protocol. No model — not even per-protocol model files — can express
//   "suppress the discriminator", "zero-byte body", or "JSON literal on XML";
//   those are serializer behaviors outside model vocabulary, so a schema
//   path here would necessarily CHANGE wire bytes.
// - Modeling them as `smithy.framework` `@error`/`@httpError` shapes and
//   routing them through `serialize_error` is the intended end-state, but it
//   is wire-changing on four of five protocols and therefore deferred until
//   the legacy-compare gates exist, so each byte diff lands as a pinned,
//   deliberate divergence. See `specs/plan.md`, Step 6 ("leaves for later").
impl IntoResponse<RestJson1> for RuntimeError {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        let runtime_error = match self {
            Self::ModeledValidation(err) => return RestJson1::serialize_error(&*err),
            runtime_error => runtime_error,
        };

        let res = http::Response::builder()
            .status(runtime_error.status_code())
            .header("Content-Type", "application/json")
            .header("X-Amzn-Errortype", runtime_error.name())
            .extension(RuntimeErrorExtension::new(runtime_error.name().to_string()));

        let body = crate::body::to_boxed("{}");

        res.body(body)
            .expect(INVALID_HTTP_RESPONSE_FOR_RUNTIME_ERROR_PANIC_MESSAGE)
    }
}

impl From<ResponseRejection> for RuntimeError {
    fn from(err: ResponseRejection) -> Self {
        Self::Serialization(crate::Error::new(err))
    }
}

impl From<RequestRejection> for RuntimeError {
    fn from(err: RequestRejection) -> Self {
        match err {
            RequestRejection::MissingContentType(_reason) => Self::UnsupportedMediaType,
            RequestRejection::ConstraintViolation(reason) => Self::Validation(reason),
            RequestRejection::SchemaConstraintViolation(reason) => Self::ModeledValidation(reason),
            RequestRejection::NotAcceptable => Self::NotAcceptable,
            _ => Self::Serialization(crate::Error::new(err)),
        }
    }
}
