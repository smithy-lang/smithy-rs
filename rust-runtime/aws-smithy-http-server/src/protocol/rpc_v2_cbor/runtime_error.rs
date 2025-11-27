/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::response::IntoResponse;
use crate::runtime_error::{InternalFailureException, INVALID_HTTP_RESPONSE_FOR_RUNTIME_ERROR_PANIC_MESSAGE};
use crate::{extension::RuntimeErrorExtension, protocol::rpc_v2_cbor::RpcV2Cbor};
use bytes::Bytes;

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
    #[error(
        "validation failure: operation input contains data that does not adhere to the modeled constraints: {0:?}"
    )]
    Validation(Vec<u8>),
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

impl IntoResponse<RpcV2Cbor> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        IntoResponse::<RpcV2Cbor>::into_response(RuntimeError::InternalFailure(crate::Error::new(String::new())))
    }
}

impl IntoResponse<RpcV2Cbor> for RuntimeError {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        let res = http::Response::builder()
            .status(self.status_code())
            .header("Content-Type", "application/cbor")
            .extension(RuntimeErrorExtension::new(self.name().to_string()));

        // https://cbor.nemo157.com/#type=hex&value=a0
        const EMPTY_CBOR_MAP: Bytes = Bytes::from_static(&[0xa0]);

        // TODO(https://github.com/smithy-lang/smithy-rs/issues/3716): we're not serializing
        // `__type`.
        let body = match self {
            RuntimeError::Validation(reason) => crate::body::to_boxed(reason),
            _ => crate::body::to_boxed(EMPTY_CBOR_MAP),
        };

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
            RequestRejection::ConstraintViolation(reason) => Self::Validation(reason),
            _ => Self::Serialization(crate::Error::new(err)),
        }
    }
}
