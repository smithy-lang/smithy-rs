/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Mounts protocol-specific request extractors on single-protocol schema services.
//! It sends accepted and rejected caller identities and checks the resulting status,
//! error header, and body, protecting the existing extractor API on the schema path.
//!
//! Protocol-specific extractors on schema services.
pub use protocol_compat::kit::{from_bytes, Body};
mod support;

use support::*;

/// A sole-protocol service's protocol type is that protocol, so a `FromParts<RestJson1>` extractor
/// keeps working and its `IntoResponse<RestJson1>` rejection is what the client receives.
#[tokio::test]
async fn sole_protocol_service_accepts_protocol_specific_extractors() {
    let ok = send(
        protocol_compat::caller::service(),
        rest("GET", "/widgets/1", &[("x-caller", "alice")], b""),
    )
    .await;
    assert_eq!(
        (ok.status, ok.text().as_str()),
        (200, r#"{"id":"alice:1"}"#)
    );

    let rejected = send(
        protocol_compat::caller::service(),
        rest("GET", "/widgets/1", &[], b""),
    )
    .await;
    assert_eq!(rejected.status, 401);
    assert_eq!(rejected.header("x-amzn-errortype"), Some("MissingCaller"));
    assert_eq!(rejected.text(), r#"{"message":"missing x-caller"}"#);
}
