/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::modeled_error::HttpModeledError;
use crate::protocol::rest_xml::RestXml;
use crate::protocol::server_protocol::ServerProtocol;
use crate::response::IntoResponse;
use crate::runtime_error::InternalFailureException;
use crate::{extension::RuntimeErrorExtension, runtime_error::INVALID_HTTP_RESPONSE_FOR_RUNTIME_ERROR_PANIC_MESSAGE};
use http::StatusCode;

use super::rejection::{RequestRejection, ResponseRejection};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// See: [`crate::protocol::rest_json_1::runtime_error::RuntimeError::Serialization`]
    #[error("request failed to deserialize or response failed to serialize: {0}")]
    Serialization(crate::Error),
    /// See: [`crate::protocol::rest_json_1::runtime_error::RuntimeError::InternalFailure`]
    #[error("internal failure: {0}")]
    InternalFailure(crate::Error),
    /// See: [`crate::protocol::rest_json_1::runtime_error::RuntimeError::NotAcceptable`]
    #[error("not acceptable request: request contains an `Accept` header with a MIME type, and the server cannot return a response body adhering to that MIME type")]
    NotAcceptable,
    /// See: [`crate::protocol::rest_json_1::runtime_error::RuntimeError::UnsupportedMediaType`]
    #[error("unsupported media type: request does not contain the expected `Content-Type` header value")]
    UnsupportedMediaType,
    /// See: [`crate::protocol::rest_json_1::runtime_error::RuntimeError::Validation`]
    #[error("validation failure: operation input contains data that does not adhere to the modeled constraints: {0}")]
    Validation(Box<dyn HttpModeledError + Send>),
}

impl RuntimeError {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Serialization(_) => "SerializationException",
            Self::InternalFailure(_) => "InternalFailureException",
            Self::NotAcceptable => "NotAcceptableException",
            Self::UnsupportedMediaType => "UnsupportedMediaTypeException",
            Self::Validation(_) => "ValidationException",
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Serialization(_) => StatusCode::BAD_REQUEST,
            Self::InternalFailure(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::NotAcceptable => StatusCode::NOT_ACCEPTABLE,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::Validation(_) => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse<RestXml> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        IntoResponse::<RestXml>::into_response(RuntimeError::InternalFailure(crate::Error::new(String::new())))
    }
}

// Only `Validation` is schema-driven: it carries a modeled shape. The other
// variants are framework conventions with no Smithy shape behind them; the
// frozen `{}` body below (a JSON literal on an XML protocol) is not the
// serialization of any shape, so they stay hand-assembled this phase. Full
// rationale on the `IntoResponse<RestJson1> for RuntimeError` impl in
// `crate::protocol::rest_json_1::runtime_error`.
impl IntoResponse<RestXml> for RuntimeError {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        // The modeled validation error serializes through the schema path —
        // exactly once, here, at the protocol boundary. (Fix-forward
        // divergence 2f: legacy restXml discarded the validation body.)
        if let RuntimeError::Validation(err) = &self {
            return RestXml::serialize_error(err.as_ref());
        }

        let res = http::Response::builder()
            .status(self.status_code())
            .header("Content-Type", "application/xml")
            .extension(RuntimeErrorExtension::new(self.name().to_string()));

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
            _ => Self::Serialization(crate::Error::new(err)),
        }
    }
}
