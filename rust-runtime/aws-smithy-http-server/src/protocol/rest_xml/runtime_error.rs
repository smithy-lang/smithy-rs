/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::protocol::rest_xml::RestXml;
use crate::response::IntoResponse;
use crate::runtime_error::InternalFailureException;
use crate::{extension::RuntimeErrorExtension, runtime_error::INVALID_HTTP_RESPONSE_FOR_RUNTIME_ERROR_PANIC_MESSAGE};
use http::StatusCode;

use super::rejection::{RequestRejection, ResponseRejection};

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
    /// [constraint traits]: <https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html>
    #[error("validation failure: operation input contains data that does not adhere to the modeled constraints: {0}")]
    Validation(String),
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

impl IntoResponse<RestXml> for RuntimeError {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
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
