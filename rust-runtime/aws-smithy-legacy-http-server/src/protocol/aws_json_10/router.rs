/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::body::{empty, BoxBody};
use crate::extension::RuntimeErrorExtension;
use crate::response::IntoResponse;
use crate::routing::{method_disallowed, UNKNOWN_OPERATION_EXCEPTION};

use super::AwsJson1_0;

pub use crate::protocol::aws_json::router::*;

// TODO(https://github.com/smithy-lang/smithy/issues/2348): We're probably non-compliant here, but
// we have no tests to pin our implemenation against!
impl IntoResponse<AwsJson1_0> for Error {
    fn into_response(self) -> http::Response<BoxBody> {
        match self {
            Error::MethodNotAllowed => method_disallowed(),
            _ => http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .header(http::header::CONTENT_TYPE, "application/x-amz-json-1.0")
                .extension(RuntimeErrorExtension::new(
                    UNKNOWN_OPERATION_EXCEPTION.to_string(),
                ))
                .body(empty())
                .expect("invalid HTTP response for AWS JSON 1.0 routing error; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues"),
        }
    }
}
