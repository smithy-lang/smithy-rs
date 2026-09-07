/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod common;

// This test invokes an operation with a client that can only send HTTP2 requests and whose TLS
// implementation is backed by `rustls`.
#[tokio::test]
async fn test_do_nothing_http2_rustls_connector() {
    let server = common::run_server().await;
    let client = common::client_http2_only(server.port);

    let _check_health = client.do_nothing().send().await.unwrap();
}
