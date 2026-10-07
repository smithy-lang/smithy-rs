/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in protocol routers, one module per protocol.

use http::Request;

fn content_type_is(request: &Request<()>, expected: &str) -> bool {
    request
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<mime::Mime>().ok())
        .is_some_and(|mime| mime.essence_str() == expected)
}

mod aws_json;
mod rest;
mod rpc_v2_cbor;

pub use aws_json::aws_json_router;
pub(crate) use rest::rest_router;
pub(crate) use rpc_v2_cbor::rpc_v2_cbor_router;
