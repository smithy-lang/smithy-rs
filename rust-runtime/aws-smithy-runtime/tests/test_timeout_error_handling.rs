/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Test that SDK correctly returns error when timeout occurs without successful HTTP response
//! 
//! This test verifies the fix for customer issue P358713763 where Lambda timeout during 
//! ELB delete_rule() incorrectly returned Ok() even though rule still existed.

#![cfg(all(feature = "client", feature = "test-util"))]

use aws_smithy_runtime::client::http::test_util::NeverClient;
use aws_smithy_runtime::client::orchestrator::operation::Operation;
use aws_smithy_runtime_api::client::interceptors::context::{Error, Output};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse, OrchestratorError};
use aws_smithy_runtime_api::client::ser_de::DeserializeResponse;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::retry::RetryConfig;
use aws_smithy_types::timeout::TimeoutConfig;
use std::time::Duration;

#[tokio::test]
async fn test_timeout_returns_error_not_ok() {
    // This test verifies the fix for the bug where SDK returned Ok() when timeout occurred.
    // After the fix, SDK correctly returns Err(TimeoutError).

    #[derive(Debug)]
    struct Deserializer;
    impl DeserializeResponse for Deserializer {
        fn deserialize_nonstreaming(
            &self,
            _resp: &HttpResponse,
        ) -> Result<Output, OrchestratorError<Error>> {
            // This should never be called if request times out
            Ok(Output::erase("success".to_owned()))
        }
    }

    // HTTP client that never responds (simulates Lambda timeout)
    let http_client = NeverClient::new();

    let op: Operation<(), String, Error> = Operation::builder()
        .service_name("test_service")
        .operation_name("test_operation")
        .http_client(http_client)
        .endpoint_url("http://localhost:1234/test")
        .no_auth()
        .standard_retry(
            &RetryConfig::standard()
                .with_max_attempts(2) // Try twice
                .with_max_backoff(Duration::from_millis(1)),
        )
        .timeout_config(
            TimeoutConfig::builder()
                .operation_attempt_timeout(Duration::from_millis(100)) // Very short timeout
                .build()
        )
        .serializer(|_body: ()| Ok(HttpRequest::new(SdkBody::empty())))
        .deserializer_impl(Deserializer)
        .build();

    // Call the operation
    let result = op.invoke(()).await;

    println!("\n=== TEST RESULT ===");
    match &result {
        Ok(_) => {
            panic!("BUG: SDK returned Ok() without successful HTTP response!");
        }
        Err(e) => {
            println!("PASS: SDK correctly returned error when timeout occurred");
            println!("Error: {e:?}");
        }
    }

    // Assert that SDK returns error when timeout occurs
    assert!(
        result.is_err(),
        "SDK must return error when timeout occurs without successful HTTP response"
    );
}
