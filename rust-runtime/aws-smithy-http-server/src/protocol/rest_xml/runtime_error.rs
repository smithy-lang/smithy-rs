/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::protocol::rest_xml::RestXml;
use crate::response::{IntoResponse, IntoResponseUniform};
use crate::runtime_error::InternalFailureException;
use crate::service::ServiceShape;
use crate::{extension::RuntimeErrorExtension, runtime_error::INVALID_HTTP_RESPONSE_FOR_RUNTIME_ERROR_PANIC_MESSAGE};
use http::StatusCode;

use super::rejection::{RequestRejection, ResponseRejection};

#[derive(Debug)]
pub enum RuntimeError {
    Serialization(crate::Error),
    InternalFailure(crate::Error),
    NotAcceptable,
    UnsupportedMediaType,
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

impl IntoResponseUniform<RestXml> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        IntoResponseUniform::<RestXml>::into_response(RuntimeError::InternalFailure(crate::Error::new(String::new())))
    }
}

impl<Ser, Op> IntoResponse<Ser, Op> for RuntimeError
where
    Ser: ServiceShape,
    Self: IntoResponseUniform<Ser::Protocol>,
{
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        IntoResponseUniform::<Ser::Protocol>::into_response(RuntimeError::InternalFailure(crate::Error::new(
            String::new(),
        )))
    }
}

impl IntoResponseUniform<RestXml> for RuntimeError {
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
