/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::rejection::MissingContentTypeReason;
use aws_smithy_runtime_api::http::HttpError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResponseRejection {
    #[error("error building HTTP response: {0}")]
    Build(#[from] aws_smithy_types::error::operation::BuildError),
    #[error("error serializing JSON-encoded body: {0}")]
    Serialization(#[from] aws_smithy_types::error::operation::SerializationError),
    #[error("error building HTTP response: {0}")]
    HttpBuild(#[from] http::Error),
}

#[derive(Debug, Error)]
pub enum RequestRejection {
    #[error("error converting non-streaming body to bytes: {0}")]
    BufferHttpBodyBytes(crate::Error),
    #[error("request contains invalid value for `Accept` header")]
    NotAcceptable,
    #[error("expected `Content-Type` header not found: {0}")]
    MissingContentType(#[from] MissingContentTypeReason),
    #[error("error deserializing request HTTP body as JSON: {0}")]
    JsonDeserialize(#[from] aws_smithy_json::deserialize::error::DeserializeError),
    #[error("request does not adhere to modeled constraints: {0}")]
    ConstraintViolation(String),

    /// Typically happens when the request has headers that are not valid UTF-8.
    #[error("failed to convert request: {0}")]
    HttpConversion(#[from] HttpError),
}

impl From<std::convert::Infallible> for RequestRejection {
    fn from(_err: std::convert::Infallible) -> Self {
        match _err {}
    }
}

convert_to_request_rejection!(hyper::Error, BufferHttpBodyBytes);
convert_to_request_rejection!(Box<dyn std::error::Error + Send + Sync + 'static>, BufferHttpBodyBytes);
