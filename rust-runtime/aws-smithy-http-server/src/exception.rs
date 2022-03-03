/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Exception types.
//!
//! This module contains "exception types". There is only one such type, [`SmithyFrameworkException`].
//!
//! As opposed to rejection types (see [`crate::rejection`]), which are an internal detail about
//! the framework, exceptions are surfaced to clients in HTTP responses: indeed, they implement
//! [`axum_core::response::IntoResponse`]. Rejections can be "grouped" and converted into an
//! exception type: for example, all request rejections due to serialization issues can be
//! conflated under the [`SmithyFrameworkExceptionType::Serialization`] exception type enum
//! variant.
//!
//! The HTTP response representation of an exception can be protocol-specific: for example,
//! exceptions in the RestJson1 protocol set the `X-Amzn-Errortype` header.
//!
//! Exception types' behavior doesn't have anything to do with how _programming language exceptions_
//! behave: there are no exceptions in Rust, `[SmithyFrameworkException`] is just a regular Rust
//! enum. Their name comes from their representation in all AWS protocols: for example,
//! [`SmithyFrameworkExceptionType::Serialization`] renders an `X-Amzn-Errortype` header with the
//! value `SerializationException` when using the RestJson1 protocol.
//!
//! The way generated code works is it always works with [`crate::rejection`] types when
//! deserializing requests and serializing response. Just before a response needs to be sent, the
//! generated code looks up and converts into the corresponding exception type, and then it uses
//! the exception's [`axum_core::response::IntoResponse`] implementation to render and send a
//! response.

use crate::protocols::Protocol;

#[derive(Debug)]
pub enum SmithyFrameworkExceptionType {
    // UnknownOperation,
    Serialization(crate::Error),
    // UnsupportedMediaType,
    // NotAcceptable,
}

/// String representation of the exception type.
/// Used as the value of the `X-Amzn-Errortype` header in RestJson1.
/// Used as the value passed to construct an [`crate::ExtensionRejection`].
impl SmithyFrameworkExceptionType {
    pub fn name(&self) -> &'static str {
        match self {
            SmithyFrameworkExceptionType::Serialization(_) => "SerializationException",
        }
    }
}

#[derive(Debug)]
pub struct SmithyFrameworkException {
    pub protocol: Protocol,
    pub exception_type: SmithyFrameworkExceptionType,
}

impl axum_core::response::IntoResponse for SmithyFrameworkException {
    fn into_response(self) -> axum_core::response::Response {
        let status_code = match self.exception_type {
            SmithyFrameworkExceptionType::Serialization(_) => http::StatusCode::BAD_REQUEST,
        };

        let headers = match self.protocol {
            Protocol::RestJson1 => [
                ("Content-Type", "application/json"),
                ("X-Amzn-Errortype", self.exception_type.name()),
            ],
        };

        let body = crate::body::to_boxed(match self.protocol {
            Protocol::RestJson1 => "{}",
        });

        let mut builder = http::Response::builder();
        builder = builder.status(status_code);
        for (header_name, header_value) in headers {
            builder = builder.header(header_name, header_value);
        }

        // TODO What extension type should we use here?
        // TODO `ResponseExtensions` should probably be renamed, as it's something
        // operation-specific. `SmithyFrameworkException` might not have reached an operation
        // (think `UnknownOperationException`).
        builder = builder.extension(crate::ResponseExtensions::new("TODO", "TODO"));

        builder.body(body).expect("invalid HTTP response for `SmithyFrameworkException`; please file a bug report under https://github.com/awslabs/smithy-rs/issues")
    }
}

impl From<crate::rejection::RequestRejection> for SmithyFrameworkExceptionType {
    fn from(err: crate::rejection::RequestRejection) -> Self {
        SmithyFrameworkExceptionType::Serialization(crate::Error::new(err))
    }
}

impl From<crate::rejection::ResponseRejection> for SmithyFrameworkExceptionType {
    fn from(err: crate::rejection::ResponseRejection) -> Self {
        SmithyFrameworkExceptionType::Serialization(crate::Error::new(err))
    }
}
