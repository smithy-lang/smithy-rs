/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod common;

use serial_test::serial;

// This test invokes an operation with a client that can only send HTTP2 requests and whose TLS
// implementation is backed by `rustls`.
#[tokio::test]
#[serial]
async fn test_do_nothing_http2_rustls_connector() {
    let _child = common::run_server().await;
    let client = common::client_http2_only();

    let _check_health = client.do_nothing().send().await.unwrap();
}

// This test invokes an operation with a client whose TLS implementation is backed by `native_tls`.
#[tokio::test]
#[serial]
async fn test_do_nothing_native_tls_connector() {
    let _child = common::run_server().await;
    let client = common::native_tls_client();

    let _check_health = client.do_nothing().send().await.unwrap();
}
