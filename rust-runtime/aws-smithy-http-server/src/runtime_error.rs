/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// A _protocol-agnostic_ type representing an internal framework error. As of writing, this can only
/// occur upon failure to extract an [`crate::extension::Extension`] from the request.
/// This type is converted into protocol-specific error variants. For example, in the
/// [`crate::protocol::rest_json_1`] protocol, it is converted to the
/// [`crate::protocol::rest_json_1::runtime_error::RuntimeError::InternalFailure`] variant.
pub struct InternalFailureException;

pub const INVALID_HTTP_RESPONSE_FOR_RUNTIME_ERROR_PANIC_MESSAGE: &str = "invalid HTTP response for `RuntimeError`; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues";

/// A protocol-independent extractor rejection framed by the selected schema protocol.
///
/// Use this error with `impl<P> FromParts<P>` extractors on single-protocol or multi-protocol
/// services. Schema upgrades call [`IntoResponse::into_response_with_protocol`], which delegates
/// to [`crate::schema::ServerProtocol::serialize_runtime_error`]. Existing protocol-specific
/// runtime-error types and their conversions remain available for explicitly static framing.
///
/// A direct [`IntoResponse::into_response`] call has no selected protocol: it returns an empty
/// response with the semantic status and a [`RuntimeErrorExtension`], without protocol headers.
/// Supply a protocol to the context-aware method when direct conversion needs protocol framing.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// Request deserialization or response serialization failed.
    #[error("request failed to deserialize or response failed to serialize: {0}")]
    Serialization(crate::error::Error),
    /// Internal framework or service state required by an extractor is unavailable.
    #[error("internal failure: {0}")]
    InternalFailure(crate::error::Error),
    /// The request's `Accept` header cannot accept the response content type.
    #[error("not acceptable request: request contains an `Accept` header with a MIME type, and the server cannot return a response body adhering to that MIME type")]
    NotAcceptable,
    /// The request's `Content-Type` does not match the expected content type.
    #[error("unsupported media type: request does not contain the expected `Content-Type` header value")]
    UnsupportedMediaType,
    /// An extractor rejected the request with a validation reason.
    ///
    /// This is a runtime validation response, distinct from a modeled constraint violation.
    /// Each protocol preserves its existing treatment of the reason, including protocols that
    /// discard it or return its raw bytes.
    #[error("validation failure: {0}")]
    Validation(String),
}

use crate::{extension::RuntimeErrorExtension, response::IntoResponse};

impl RuntimeError {
    /// The error name used by protocol framing and [`RuntimeErrorExtension`].
    pub fn name(&self) -> &'static str {
        match self {
            Self::Serialization(_) => "SerializationException",
            Self::InternalFailure(_) => "InternalFailureException",
            Self::NotAcceptable => "NotAcceptableException",
            Self::UnsupportedMediaType => "UnsupportedMediaTypeException",
            Self::Validation(_) => "ValidationException",
        }
    }

    /// The semantic status used when conversion has no selected protocol.
    pub fn status_code(&self) -> http::StatusCode {
        match self {
            Self::Serialization(_) | Self::Validation(_) => http::StatusCode::BAD_REQUEST,
            Self::InternalFailure(_) => http::StatusCode::INTERNAL_SERVER_ERROR,
            Self::NotAcceptable => http::StatusCode::NOT_ACCEPTABLE,
            Self::UnsupportedMediaType => http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
        }
    }
}

impl<P> IntoResponse<P> for RuntimeError {
    fn into_response(self) -> crate::response::Response {
        let mut response = http::Response::new(crate::body::empty());
        *response.status_mut() = self.status_code();
        response
            .extensions_mut()
            .insert(RuntimeErrorExtension::new(self.name().to_owned()));
        response
    }

    fn into_response_with_protocol(self, protocol: &dyn crate::schema::ServerProtocol) -> crate::response::Response {
        protocol.serialize_runtime_error(self)
    }
}

// Reuse each protocol's established response conversion, including its wire quirks. CBOR's
// validation variant stores raw bytes; the other protocols store the reason as a string.
macro_rules! protocol_runtime_error {
    ($error:ty) => {
        impl From<RuntimeError> for $error {
            fn from(error: RuntimeError) -> Self {
                match error {
                    RuntimeError::Serialization(error) => Self::Serialization(error),
                    RuntimeError::InternalFailure(error) => Self::InternalFailure(error),
                    RuntimeError::NotAcceptable => Self::NotAcceptable,
                    RuntimeError::UnsupportedMediaType => Self::UnsupportedMediaType,
                    RuntimeError::Validation(reason) => Self::Validation(reason.into()),
                }
            }
        }
    };
}
protocol_runtime_error!(crate::protocol::rest_json_1::runtime_error::RuntimeError);
protocol_runtime_error!(crate::protocol::aws_json::runtime_error::RuntimeError);
protocol_runtime_error!(crate::protocol::rest_xml::runtime_error::RuntimeError);
protocol_runtime_error!(crate::protocol::rpc_v2_cbor::runtime_error::RuntimeError);
