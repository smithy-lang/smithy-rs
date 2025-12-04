/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_cloudfront_url_signer::{sign_cookies, sign_url, PrivateKey, SigningRequest};
use aws_smithy_types::DateTime;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct TestCase {
    id: String,
    documentation: String,
    input: TestInput,
    expected: TestExpected,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestInput {
    resource_url: String,
    key_pair_id: String,
    private_key_file: String,
    expiration_date: Option<i64>,
    active_date: Option<i64>,
    ip_range: Option<String>,
    resource_url_pattern: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestExpected {
    #[allow(dead_code)]
    policy_json: Option<String>,
    query_params: Option<HashMap<String, String>>,
    cookies: Option<HashMap<String, String>>,
    signature: Option<String>,
    signature_algorithm: Option<String>,
    error: Option<bool>,
    error_contains: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct TestCases {
    #[serde(flatten)]
    #[allow(dead_code)]
    cases: Vec<TestCase>,
}

fn load_test_cases() -> Vec<TestCase> {
    let json = include_str!("test-cases.json");
    serde_json::from_str(json).expect("Failed to parse test cases")
}

fn parse_url_query_params(url: &str) -> HashMap<String, String> {
    url.split('?')
        .nth(1)
        .map(|query| {
            query
                .split('&')
                .filter_map(|pair| {
                    let mut parts = pair.split('=');
                    Some((parts.next()?.to_string(), parts.next()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn test_sep_test_cases() {
    let test_cases = load_test_cases();

    for test_case in test_cases {
        println!(
            "\nRunning test: {} - {}",
            test_case.id, test_case.documentation
        );

        // Load private key
        let key_path = format!("tests/{}", test_case.input.private_key_file);
        let key_bytes = std::fs::read(&key_path)
            .unwrap_or_else(|_| panic!("Failed to read key file: {key_path}"));
        let private_key = PrivateKey::from_pem(&key_bytes)
            .unwrap_or_else(|_| panic!("Failed to parse private key for test {}", test_case.id));

        // Build signing request
        let mut builder = SigningRequest::builder()
            .resource_url(&test_case.input.resource_url)
            .key_pair_id(&test_case.input.key_pair_id)
            .private_key(private_key);

        if let Some(exp) = test_case.input.expiration_date {
            builder = builder.expires_at(DateTime::from_secs(exp));
        }

        if let Some(active) = test_case.input.active_date {
            builder = builder.active_at(DateTime::from_secs(active));
        }

        if let Some(ip) = &test_case.input.ip_range {
            builder = builder.ip_range(ip);
        }

        if let Some(pattern) = &test_case.input.resource_url_pattern {
            builder = builder.resource_pattern(pattern);
        }

        // Handle error cases
        if test_case.expected.error == Some(true) {
            let result = builder.build();
            assert!(
                result.is_err(),
                "Test {} expected error but succeeded",
                test_case.id
            );

            if let Some(error_contains) = &test_case.expected.error_contains {
                let error_msg = result.unwrap_err().to_string().to_lowercase();
                for expected_text in error_contains {
                    assert!(
                        error_msg.contains(&expected_text.to_lowercase()),
                        "Test {} error message '{}' does not contain '{}'",
                        test_case.id,
                        error_msg,
                        expected_text
                    );
                }
            }
            continue;
        }

        let request = builder
            .build()
            .unwrap_or_else(|e| panic!("Failed to build request for test {}: {}", test_case.id, e));

        // Determine if this is a URL or cookie test
        let is_cookie_test = test_case.expected.cookies.is_some();

        if is_cookie_test {
            // Test signed cookies
            let cookies = sign_cookies(request).unwrap_or_else(|e| {
                panic!("Failed to sign cookies for test {}: {}", test_case.id, e)
            });

            let cookie_map: HashMap<String, String> = cookies
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();

            // Verify expected cookies
            if let Some(expected_cookies) = &test_case.expected.cookies {
                for (key, expected_value) in expected_cookies {
                    let actual_value = cookie_map
                        .get(key)
                        .unwrap_or_else(|| panic!("Test {} missing cookie: {}", test_case.id, key));

                    assert_eq!(
                        actual_value, expected_value,
                        "Test {} cookie {} mismatch",
                        test_case.id, key
                    );
                }
            }
        } else {
            // Test signed URL
            let signed_url = sign_url(request)
                .unwrap_or_else(|e| panic!("Failed to sign URL for test {}: {}", test_case.id, e));

            let query_params = parse_url_query_params(signed_url.url());

            // Verify expected query parameters
            if let Some(expected_params) = &test_case.expected.query_params {
                for (key, expected_value) in expected_params {
                    let actual_value = query_params.get(key).unwrap_or_else(|| {
                        panic!("Test {} missing query param: {}", test_case.id, key)
                    });

                    assert_eq!(
                        actual_value, expected_value,
                        "Test {} query param {} mismatch",
                        test_case.id, key
                    );
                }
            }

            // Verify signature if provided (skip for ECDSA as it may not be deterministic)
            if let Some(expected_signature) = &test_case.expected.signature {
                if test_case.expected.signature_algorithm.as_deref() != Some("ECDSA-SHA1") {
                    let actual_signature = query_params.get("Signature").unwrap_or_else(|| {
                        panic!("Test {} missing Signature query param", test_case.id)
                    });

                    assert_eq!(
                        actual_signature, expected_signature,
                        "Test {} signature mismatch",
                        test_case.id
                    );
                } else {
                    // For ECDSA, just verify signature is present
                    assert!(
                        query_params.contains_key("Signature"),
                        "Test {} missing Signature query param",
                        test_case.id
                    );
                }
            }
        }

        println!("✓ Test {} passed", test_case.id);
    }
}
