/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in protocol routers, one module per protocol.

use http::Request;

use super::OperationIndex;

fn content_type_is(request: &Request<()>, expected: &str) -> bool {
    request
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<mime::Mime>().ok())
        .is_some_and(|mime| mime.essence_str() == expected)
}

/// Whether the request head announces an empty body.
fn announces_no_body(request: &Request<()>) -> bool {
    let headers = request.headers();
    !headers.contains_key(http::header::TRANSFER_ENCODING)
        && headers
            .get(http::header::CONTENT_LENGTH)
            .is_none_or(|length| length.as_bytes() == b"0")
}

/// The `Content-Type` a REST protocol requires to claim a request for one operation.

fn per_target<T>(
    targets: &[OperationIndex],
    default: impl Fn() -> T,
    mut value: impl FnMut(OperationIndex) -> T,
) -> Vec<T> {
    let len = targets.iter().map(|target| target.index + 1).max().unwrap_or(0);
    let mut table: Vec<T> = (0..len).map(|_| default()).collect();
    for target in targets {
        table[target.index] = value(*target);
    }
    table
}


mod aws_json;
mod rest;
mod rpc_v2_cbor;

pub use aws_json::aws_json_router;
pub(crate) use rest::rest_router;
pub(crate) use rpc_v2_cbor::rpc_v2_cbor_router;

