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
//! [`crate::response::IntoResponse`]. Rejections can be "grouped" and converted into a
//! specific `RuntimeError` kind: for example, all request rejections due to serialization issues
//! can be conflated under the [`RuntimeErrorKind::Serialization`] enum variant.
//!
//! The HTTP response representation of the specific `RuntimeError` can be protocol-specific: for
//! example, the runtime error in the RestJson1 protocol sets the `X-Amzn-Errortype` header.
//!
//! Generated code works always works with [`crate::rejection`] types when deserializing requests
//! and serializing response. Just before a response needs to be sent, the generated code looks up
//! and converts into the corresponding `RuntimeError`, and then it uses the its
//! [`crate::response::IntoResponse`] implementation to render and send a response.

use crate::{
    protocols::Protocol,
    response::{IntoResponse, Response},
};

#[derive(Debug)]
pub enum RuntimeErrorKind {
    /// The requested operation does not exist.
    UnknownOperation,
    /// Request failed to deserialize or response failed to serialize.
    Serialization(crate::Error),
    /// As of writing, this variant can only occur upon failure to extract an
    /// [`crate::extension::Extension`] from the request.
    InternalFailure(crate::Error),
    // UnsupportedMediaType,
    NotAcceptable,
}

/// String representation of the runtime error type.
/// Used as the value of the `X-Amzn-Errortype` header in RestJson1.
/// Used as the value passed to construct an [`crate::extension::RuntimeErrorExtension`].
impl RuntimeErrorKind {
    pub fn name(&self) -> &'static str {
        match self {
            RuntimeErrorKind::Serialization(_) => "SerializationException",
            RuntimeErrorKind::InternalFailure(_) => "InternalFailureException",
            RuntimeErrorKind::UnknownOperation => "UnknownOperationException",
            RuntimeErrorKind::NotAcceptable => "NotAcceptableException",
        }
    }
}

#[derive(Debug)]
pub struct RuntimeError {
    pub protocol: Protocol,
    pub kind: RuntimeErrorKind,
}

impl IntoResponse for RuntimeError {
    fn into_response(self) -> Response {
        let status_code = match self.kind {
            RuntimeErrorKind::Serialization(_) => http::StatusCode::BAD_REQUEST,
            RuntimeErrorKind::InternalFailure(_) => http::StatusCode::INTERNAL_SERVER_ERROR,
            RuntimeErrorKind::UnknownOperation => http::StatusCode::NOT_FOUND,
            RuntimeErrorKind::NotAcceptable => http::StatusCode::NOT_ACCEPTABLE,
        };

        let body = crate::body::to_boxed(match self.protocol {
            Protocol::RestJson1 => "{}",
            Protocol::RestXml => "",
            // See https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_0-protocol.html#empty-body-serialization
            Protocol::AwsJson10 => "",
            // See https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_1-protocol.html#empty-body-serialization
            Protocol::AwsJson11 => "",
        });

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
        RuntimeErrorKind::Serialization(crate::Error::new(err))
    }
}
