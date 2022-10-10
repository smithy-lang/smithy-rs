/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime error type.
//!
//! This module contains [`RuntimeError`] type.
//!
//! As opposed to rejection types (see [`crate::rejection`]), which are an internal detail about
//! the framework, `RuntimeError` is surfaced to clients in HTTP responses: indeed, it implements
//! [`RuntimeError::into_response`]. Rejections can be "grouped" and converted into a
//! specific `RuntimeError` kind: for example, all request rejections due to serialization issues
//! can be conflated under the [`RuntimeError::Serialization`] enum variant.
//!
//! The HTTP response representation of the specific `RuntimeError` can be protocol-specific: for
//! example, the runtime error in the RestJson1 protocol sets the `X-Amzn-Errortype` header.
//!
//! Generated code works always works with [`crate::rejection`] types when deserializing requests
//! and serializing response. Just before a response needs to be sent, the generated code looks up
//! and converts into the corresponding `RuntimeError`, and then it uses the its
//! [`RuntimeError::into_response`] method to render and send a response.

use http::StatusCode;

use crate::extension::RuntimeErrorExtension;
use crate::proto::aws_json_10::AwsJson10;
use crate::proto::aws_json_11::AwsJson11;
use crate::proto::rest_json_1::AwsRestJson1;
use crate::proto::rest_xml::AwsRestXml;
use crate::response::IntoResponse;

#[derive(Debug)]
pub enum RuntimeError {
    /// Request failed to deserialize or response failed to serialize.
    Serialization(crate::Error),
    /// As of writing, this variant can only occur upon failure to extract an
    /// [`crate::extension::Extension`] from the request.
    InternalFailure(crate::Error),
    // TODO(https://github.com/awslabs/smithy-rs/issues/1663)
    NotAcceptable,
    UnsupportedMediaType,
}

/// String representation of the runtime error type.
/// Used as the value of the `X-Amzn-Errortype` header in RestJson1.
/// Used as the value passed to construct an [`crate::extension::RuntimeErrorExtension`].
impl RuntimeError {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Serialization(_) => "SerializationException",
            Self::InternalFailure(_) => "InternalFailureException",
            Self::NotAcceptable => "NotAcceptableException",
            Self::UnsupportedMediaType => "UnsupportedMediaTypeException",
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Serialization(_) => StatusCode::BAD_REQUEST,
            Self::InternalFailure(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::NotAcceptable => StatusCode::NOT_ACCEPTABLE,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        }
    }
}

pub struct InternalFailureException;

impl IntoResponse<AwsJson10> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        IntoResponse::<AwsJson10>::into_response(RuntimeError::InternalFailure(crate::Error::new(String::new())))
    }
}

impl IntoResponse<AwsJson11> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        IntoResponse::<AwsJson11>::into_response(RuntimeError::InternalFailure(crate::Error::new(String::new())))
    }
}

impl IntoResponse<AwsRestJson1> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        IntoResponse::<AwsRestJson1>::into_response(RuntimeError::InternalFailure(crate::Error::new(String::new())))
    }
}

impl IntoResponse<AwsRestXml> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        IntoResponse::<AwsRestXml>::into_response(RuntimeError::InternalFailure(crate::Error::new(String::new())))
    }
}

impl IntoResponse<AwsRestJson1> for RuntimeError {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        http::Response::builder()
            .status(self.status_code())
            .header("Content-Type", "application/json")
            .header("X-Amzn-Errortype", self.name())
            .extension(RuntimeErrorExtension::new(self.name().to_string()))
            // See https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_1-protocol.html#empty-body-serialization
            .body(crate::body::to_boxed("{}"))
            .expect("invalid HTTP response for `RuntimeError`; please file a bug report under https://github.com/awslabs/smithy-rs/issues")
    }
}

impl IntoResponse<AwsRestXml> for RuntimeError {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        http::Response::builder()
            .status(self.status_code())
            .header("Content-Type", "application/xml")
            .extension(RuntimeErrorExtension::new(self.name().to_string()))
            .body(crate::body::to_boxed(""))
            .expect("invalid HTTP response for `RuntimeError`; please file a bug report under https://github.com/awslabs/smithy-rs/issues")
    }
}

impl IntoResponse<AwsJson10> for RuntimeError {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        http::Response::builder()
            .status(self.status_code())
            .header("Content-Type", "application/x-amz-json-1.0")
            .extension(RuntimeErrorExtension::new(self.name().to_string()))
            // See https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_0-protocol.html#empty-body-serialization
            .body(crate::body::to_boxed(""))
            .expect("invalid HTTP response for `RuntimeError`; please file a bug report under https://github.com/awslabs/smithy-rs/issues")
    }
}

impl IntoResponse<AwsJson11> for RuntimeError {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        http::Response::builder()
            .status(self.status_code())
            .header("Content-Type", "application/x-amz-json-1.1")
            .extension(RuntimeErrorExtension::new(self.name().to_string()))
            // See https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_1-protocol.html#empty-body-serialization
            .body(crate::body::to_boxed(""))
            .expect("invalid HTTP response for `RuntimeError`; please file a bug report under https://github.com/awslabs/smithy-rs/issues")
    }
}

impl From<crate::rejection::RequestExtensionNotFoundRejection> for RuntimeError {
    fn from(err: crate::rejection::RequestExtensionNotFoundRejection) -> Self {
        Self::InternalFailure(crate::Error::new(err))
    }
}

impl From<crate::rejection::ResponseRejection> for RuntimeError {
    fn from(err: crate::rejection::ResponseRejection) -> Self {
        Self::Serialization(crate::Error::new(err))
    }
}

impl From<crate::rejection::RequestRejection> for RuntimeError {
    fn from(err: crate::rejection::RequestRejection) -> Self {
        match err {
            crate::rejection::RequestRejection::MissingContentType(_reason) => Self::UnsupportedMediaType,
            _ => Self::Serialization(crate::Error::new(err)),
        }
    }
}
