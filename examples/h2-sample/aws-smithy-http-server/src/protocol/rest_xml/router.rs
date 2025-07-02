/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::body::empty;
use crate::body::BoxBody;
use crate::extension::RuntimeErrorExtension;
use crate::response::IntoResponse;
use crate::routing::{method_disallowed, UNKNOWN_OPERATION_EXCEPTION};

use super::RestXml;

pub use crate::protocol::rest::router::*;

// TODO(https://github.com/smithy-lang/smithy/issues/2348): We're probably non-compliant here, but
// we have no tests to pin our implemenation against!
impl IntoResponse<RestXml> for Error {
    fn into_response(self) -> http::Response<BoxBody> {
        match self {
            Error::NotFound => http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .header(http::header::CONTENT_TYPE, "application/xml")
                .extension(RuntimeErrorExtension::new(
                    UNKNOWN_OPERATION_EXCEPTION.to_string(),
                ))
                .body(empty())
                .expect("invalid HTTP response for REST XML routing error; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues"),
            Error::MethodNotAllowed => method_disallowed(),
        }
    }
}
