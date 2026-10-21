/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Discards selected-operation context in request middleware and calls the generated
//! schema service. A control request succeeds, while the modified request must return
//! the pinned bare 500 response when the upgrade layer cannot recover routing context.
//!
//! The upgrade layer answers discarded routing context with a bare 500.
pub use protocol_compat::kit::{from_bytes, Body};
mod support;

use protocol_compat::tampered;
use support::*;

#[tokio::test]
async fn missing_operation_selection_response_is_pinned() {
    let service = tampered::service();
    let ok = send(service.clone(), rest("GET", "/widgets/1", &[], b"")).await;
    assert_eq!(ok.status, 200);

    let reply = send(service, rest("POST", "/ping", &[("x-tamper", "drop")], b"")).await;
    // Context loss retains the existing bare 500, rather than the protocol-framed internal failure.
    assert_eq!(
        (reply.status, reply.headers.len(), reply.body.len()),
        (500, 0, 0)
    );
}
