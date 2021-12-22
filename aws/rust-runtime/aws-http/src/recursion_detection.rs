/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::recursion_detection::env::TRACE_ID;
use aws_smithy_http::middleware::MapRequest;
use aws_smithy_http::operation::Request;
use aws_types::os_shim_internal::Env;
use http::HeaderValue;
use percent_encoding::{percent_encode, CONTROLS};
use std::borrow::Cow;

/// Recursion Detection Middleware
///
/// This middleware inspects the value of the `AWS_LAMBDA_FUNCTION_NAME` and `_X_AMZ_TRACE_ID` environment
/// variables to detect if the request is being invoked in a lambda function. If it is, the `X-Amzn-Trace-Id` header
/// will be set. This enables downstream services to prevent accidentally infinitely recursive invocations spawned
/// from lambda.
#[non_exhaustive]
#[derive(Default, Debug, Clone)]
pub struct RecursionDetectionStage {
    env: Env,
}

impl RecursionDetectionStage {
    /// Creates a new `RecursionDetectionStage`
    pub fn new() -> Self {
        Self::default()
    }
}

impl MapRequest for RecursionDetectionStage {
    type Error = std::convert::Infallible;

    fn apply(&self, request: Request) -> Result<Request, Self::Error> {
        request.augment(|mut req, _conf| {
            augument_request(&mut req, &self.env);
            Ok(req)
        })
    }
}

const TRACE_ID_HEADER: &str = "x-amzn-trace-id";

mod env {
    pub(super) const LAMBDA_FUNCTION_NAME: &str = "AWS_LAMBDA_FUNCTION_NAME";
    pub(super) const TRACE_ID: &str = "_X_AMZ_TRACE_ID";
}

fn augument_request<B>(req: &mut http::Request<B>, env: &Env) {
    if req.headers().contains_key(TRACE_ID_HEADER) {
        return;
    }
    if let (Ok(_function_name), Ok(trace_id)) =
        (env.get(env::LAMBDA_FUNCTION_NAME), env.get(TRACE_ID))
    {
        req.headers_mut()
            .insert(TRACE_ID_HEADER, encode_header(trace_id.as_bytes()));
    }
}

/// Encodes a byte slice as a header.
///
/// ASCII control characters are percent encoded which ensures that all byte sequences are valid headers
fn encode_header(value: &[u8]) -> HeaderValue {
    let value: Cow<'_, str> = percent_encode(value, &CONTROLS).into();
    HeaderValue::from_bytes(value.as_bytes()).expect("header is encoded, header must be valid")
}

#[cfg(test)]
mod test {
    use crate::recursion_detection::{encode_header, RecursionDetectionStage};
    use aws_smithy_http::body::SdkBody;
    use aws_smithy_http::middleware::MapRequest;
    use aws_smithy_http::operation;
    use aws_smithy_protocol_test::{assert_ok, validate_headers};
    use aws_types::os_shim_internal::Env;
    use proptest::{prelude::*, proptest};
    use serde::Deserialize;
    use std::collections::HashMap;

    proptest! {
        #[test]
        fn header_encoding_never_panics(s in any::<Vec<u8>>()) {
            encode_header(&s);
        }
    }

    #[test]
    fn run_tests() {
        let test_cases: Vec<TestCase> =
            serde_json::from_str(include_str!("../test-data/recursion-detection.json"))
                .expect("invalid test case");
        for test_case in test_cases {
            check(test_case)
        }
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct TestCase {
        env: HashMap<String, String>,
        request_headers_before: Vec<String>,
        request_headers_after: Vec<String>,
    }
    impl TestCase {
        fn env(&self) -> Env {
            Env::from(self.env.clone())
        }

        fn request_headers_before(&self) -> impl Iterator<Item = (&str, &str)> {
            self.request_headers_before
                .iter()
                .map(|header| header.split_once(": ").expect("header must contain :"))
        }

        fn request_headers_after(&self) -> impl Iterator<Item = (&str, &str)> {
            self.request_headers_after
                .iter()
                .map(|header| header.split_once(": ").expect("header must contain :"))
        }
    }

    fn check(test_case: TestCase) {
        let env = test_case.env();
        let mut req = http::Request::builder();
        for (k, v) in test_case.request_headers_before() {
            req = req.header(k, v);
        }
        let req = req.body(SdkBody::empty()).expect("must be valid");
        let req = operation::Request::new(req);
        let augmented_req = RecursionDetectionStage { env }
            .apply(req)
            .expect("stage must succeed");
        for k in augmented_req.http().headers().keys() {
            assert_eq!(
                augmented_req.http().headers().get_all(k).iter().count(),
                1,
                "No duplicated headers"
            )
        }
        assert_ok(validate_headers(
            &augmented_req.http(),
            test_case.request_headers_after(),
        ))
    }
}
