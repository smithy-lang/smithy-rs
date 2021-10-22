/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use http::HeaderValue;
use smithy_client::dvr::RecordingConnection;

const NAUGHTY_STRINGS: &str = include_str!("../../blns.txt");

#[tokio::test]
async fn test_client_context_field_against_naughty_strings_list() {
    tracing_subscriber::fmt::init();

    let config = aws_config::load_from_env().await;
    let client = aws_sdk_lambda::Client::new(&config);
    let invalid_request_content_exception = "InvalidRequestContentException: Client context must be a valid Base64-encoded JSON object.";

    for (idx, line) in NAUGHTY_STRINGS.split('\n').enumerate().skip(123) {
        // add lines to metadata unless they're a comment or empty
        // Some naughty strings aren't valid HeaderValues so we skip those too
        if !line.starts_with("#") && !line.is_empty() && HeaderValue::from_str(line).is_ok() {
            let err = client
                .invoke()
                .function_name("testFunctionThatDoesNothing")
                .client_context(line)
                .send()
                .await
                .unwrap_err();

            if err.to_string() != invalid_request_content_exception {
                // 1 is added to idx because line numbers start at one
                panic!(
                    "line {} '{}' caused unexpected error: {}",
                    idx + 1,
                    line,
                    err
                );
            };
        }
    }
}
