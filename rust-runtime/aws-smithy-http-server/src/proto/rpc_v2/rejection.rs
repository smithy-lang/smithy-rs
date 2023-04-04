/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use strum_macros::Display;

use crate::rejection::MissingContentTypeReason;

#[derive(Debug, Display)]
pub enum ResponseRejection {
    InvalidHttpStatusCode,
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
    CborDeserialize(crate::Error),
    // Unlike the other protocols, RPC v2 uses CBOR, a binary serialization format, so we take in a
    // `Vec<u8>` here instead of `String`.
    ConstraintViolation(Vec<u8>),
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

convert_to_request_rejection!(aws_smithy_cbor::decode::DeserializeError, CborDeserialize);

convert_to_request_rejection!(hyper::Error, HttpBody);

convert_to_request_rejection!(Box<dyn std::error::Error + Send + Sync + 'static>, HttpBody);
