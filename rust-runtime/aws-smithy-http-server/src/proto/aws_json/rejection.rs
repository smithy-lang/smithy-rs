/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use strum_macros::Display;

use crate::rejection::MissingContentTypeReason;

#[derive(Debug, Display)]
pub enum ResponseRejection {
    Serialization(crate::Error),
    Http(crate::Error),
}

impl std::error::Error for ResponseRejection {}

convert_to_response_rejection!(aws_smithy_http::operation::error::SerializationError, Serialization);
convert_to_response_rejection!(http::Error, Http);

#[derive(Debug, Display)]
pub enum RequestRejection {
    HttpBody(crate::Error),
    MissingContentType(MissingContentTypeReason),
    JsonDeserialize(crate::Error),
    ConstraintViolation(String),
}

impl std::error::Error for RequestRejection {}

impl From<std::convert::Infallible> for RequestRejection {
    fn from(_err: std::convert::Infallible) -> Self {
        match _err {}
    }
}

impl From<MissingContentTypeReason> for RequestRejection {
    fn from(e: MissingContentTypeReason) -> Self {
        Self::MissingContentType(e)
    }
}

convert_to_request_rejection!(aws_smithy_json::deserialize::error::DeserializeError, JsonDeserialize);

convert_to_request_rejection!(hyper::Error, HttpBody);

convert_to_request_rejection!(Box<dyn std::error::Error + Send + Sync + 'static>, HttpBody);
