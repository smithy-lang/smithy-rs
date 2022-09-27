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
//! can be conflated under the [`RuntimeErrorKind::Serialization`] enum variant.
//!
//! The HTTP response representation of the specific `RuntimeError` can be protocol-specific: for
//! example, the runtime error in the RestJson1 protocol sets the `X-Amzn-Errortype` header.
//!
//! Generated code works always works with [`crate::rejection`] types when deserializing requests
//! and serializing response. Just before a response needs to be sent, the generated code looks up
//! and converts into the corresponding `RuntimeError`, and then it uses the its
//! [`RuntimeError::into_response`] method to render and send a response.

use std::borrow::Cow;

use crate::{
    protocols::{AwsJson10, AwsJson11, AwsRestJson1, AwsRestXml, Protocol},
    response::{IntoResponse, Response},
};

#[derive(Debug)]
pub enum RuntimeErrorKind {
    /// Request failed to deserialize or response failed to serialize.
    Serialization(crate::Error),
    /// As of writing, this variant can only occur upon failure to extract an
    /// [`crate::extension::Extension`] from the request.
    InternalFailure(crate::Error),
    // TODO(https://github.com/awslabs/smithy-rs/issues/1663)
    NotAcceptable,
    UnsupportedMediaType,

    // TODO(https://github.com/awslabs/smithy-rs/issues/1703): this will hold a type that can be
    // rendered into a protocol-specific response later on.
    Validation(String),
}

/// String representation of the runtime error type.
/// Used as the value of the `X-Amzn-Errortype` header in RestJson1.
/// Used as the value passed to construct an [`crate::extension::RuntimeErrorExtension`].
impl RuntimeErrorKind {
    pub fn name(&self) -> &'static str {
        match self {
            RuntimeErrorKind::Serialization(_) => "SerializationException",
            RuntimeErrorKind::InternalFailure(_) => "InternalFailureException",
            RuntimeErrorKind::NotAcceptable => "NotAcceptableException",
            RuntimeErrorKind::UnsupportedMediaType => "UnsupportedMediaTypeException",
            RuntimeErrorKind::Validation(_) => "ValidationException",
        }
    }
}

pub struct InternalFailureException;

impl IntoResponse<AwsJson10> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        RuntimeError::internal_failure_from_protocol(Protocol::AwsJson10).into_response()
    }
}

impl IntoResponse<AwsJson11> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        RuntimeError::internal_failure_from_protocol(Protocol::AwsJson11).into_response()
    }
}

impl IntoResponse<AwsRestJson1> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        RuntimeError::internal_failure_from_protocol(Protocol::RestJson1).into_response()
    }
}

impl IntoResponse<AwsRestXml> for InternalFailureException {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        RuntimeError::internal_failure_from_protocol(Protocol::RestXml).into_response()
    }
}

#[derive(Debug)]
pub struct RuntimeError {
    pub protocol: Protocol,
    pub kind: RuntimeErrorKind,
}

impl<P> IntoResponse<P> for RuntimeError {
    fn into_response(self) -> http::Response<crate::body::BoxBody> {
        self.into_response()
    }
}

impl RuntimeError {
    pub fn internal_failure_from_protocol(protocol: Protocol) -> Self {
        RuntimeError {
            protocol,
            kind: RuntimeErrorKind::InternalFailure(crate::Error::new(String::new())),
        }
    }

    pub fn into_response(self) -> Response {
        let status_code = match self.kind {
            RuntimeErrorKind::Serialization(_) => http::StatusCode::BAD_REQUEST,
            RuntimeErrorKind::InternalFailure(_) => http::StatusCode::INTERNAL_SERVER_ERROR,
            RuntimeErrorKind::NotAcceptable => http::StatusCode::NOT_ACCEPTABLE,
            RuntimeErrorKind::UnsupportedMediaType => http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
            RuntimeErrorKind::Validation(_) => http::StatusCode::BAD_REQUEST,
        };

        let mut builder = http::Response::builder();
        builder = builder.status(status_code);

        match self.protocol {
            Protocol::RestJson1 => {
                builder = builder
                    .header("Content-Type", "application/json")
                    .header("X-Amzn-Errortype", self.kind.name());
            }
            Protocol::RestXml => builder = builder.header("Content-Type", "application/xml"),
            Protocol::AwsJson10 => builder = builder.header("Content-Type", "application/x-amz-json-1.0"),
            Protocol::AwsJson11 => builder = builder.header("Content-Type", "application/x-amz-json-1.1"),
        }

        builder = builder.extension(crate::extension::RuntimeErrorExtension::new(String::from(
            self.kind.name(),
        )));

        let body = crate::body::to_boxed(match self.kind {
            RuntimeErrorKind::Validation(reason) => Cow::Owned(match self.protocol {
                Protocol::RestJson1 | Protocol::AwsJson10 | Protocol::AwsJson11 => {
                    // https://docs.rs/serde_json/latest/serde_json/ser/fn.to_string.html#errors
                    // serde_json::to_string(reason).expect("unexpected failure during serialization of constraint violation; please file a bug report under https://github.com/awslabs/smithy-rs/issues")
                    reason
                }
                Protocol::RestXml => todo!(),
            }),
            _ => {
                Cow::Borrowed(match self.protocol {
                    Protocol::RestJson1 => "{}",
                    Protocol::RestXml => "",
                    // See https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_0-protocol.html#empty-body-serialization
                    Protocol::AwsJson10 => "",
                    // See https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_1-protocol.html#empty-body-serialization
                    Protocol::AwsJson11 => "",
                })
            }
        });

        builder.body(body).expect("invalid HTTP response for `RuntimeError`; please file a bug report under https://github.com/awslabs/smithy-rs/issues")
    }
}

impl From<crate::rejection::RequestExtensionNotFoundRejection> for RuntimeErrorKind {
    fn from(err: crate::rejection::RequestExtensionNotFoundRejection) -> Self {
        RuntimeErrorKind::InternalFailure(crate::Error::new(err))
    }
}

impl From<crate::rejection::ResponseRejection> for RuntimeErrorKind {
    fn from(err: crate::rejection::ResponseRejection) -> Self {
        RuntimeErrorKind::Serialization(crate::Error::new(err))
    }
}

impl From<crate::rejection::RequestRejection> for RuntimeErrorKind {
    fn from(err: crate::rejection::RequestRejection) -> Self {
        match err {
            crate::rejection::RequestRejection::MissingContentType(_reason) => RuntimeErrorKind::UnsupportedMediaType,
            crate::rejection::RequestRejection::ConstraintViolation(reason) => RuntimeErrorKind::Validation(reason),
            _ => RuntimeErrorKind::Serialization(crate::Error::new(err)),
        }
    }
}
