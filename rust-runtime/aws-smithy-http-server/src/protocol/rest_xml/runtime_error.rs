/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::protocol::rest_xml::RestXml;
use crate::response::IntoResponse;
use crate::runtime_error::InternalFailureException;
use crate::{
    extension::RuntimeErrorExtension, modeled_error::HttpModeledError,
    runtime_error::INVALID_HTTP_RESPONSE_FOR_RUNTIME_ERROR_PANIC_MESSAGE, schema::protocol::ServerProtocol,
};
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
    Validation(String),
    /// See: [`crate::protocol::rest_json_1::runtime_error::RuntimeError::ModeledValidation`]
    #[error("validation failure: operation input contains data that does not adhere to the modeled constraints: {0}")]
    ModeledValidation(Box<dyn HttpModeledError + Send>),
}

impl RuntimeError {
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

impl IntoResponse<RestXml> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        IntoResponse::<RestXml>::into_response(RuntimeError::InternalFailure(crate::Error::new(String::new())))
    }
}

// Only `ModeledValidation` is schema-driven: it carries a modeled shape. The
// legacy `Validation` variant and the other framework variants have frozen
// hand-assembled wire forms; the `{}` body below (a JSON literal on an XML
// protocol) is not the serialization of any shape, so they stay hand-assembled
// this phase. Full rationale on the `IntoResponse<RestJson1> for
// RuntimeError` impl in `crate::protocol::rest_json_1::runtime_error`.
impl IntoResponse<RestXml> for RuntimeError {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        let runtime_error = match self {
            Self::ModeledValidation(err) => return RestXml::serialize_error(&*err),
            runtime_error => runtime_error,
        };

        let res = http::Response::builder()
            .status(runtime_error.status_code())
            .header("Content-Type", "application/xml")
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
            _ => Self::Serialization(crate::Error::new(err)),
        }
    }
}
