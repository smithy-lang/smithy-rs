/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Mock response types for aws-smithy-mocks.

use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::http::StatusCode;
use aws_smithy_types::body::SdkBody;

/// A mock response that can be returned by a rule.
///
/// This enum represents the different types of responses that can be returned by a mock rule:
/// - `Output`: A successful modeled response
/// - `Error`: A modeled error
/// - `Http`: An HTTP response
///
#[derive(Debug)]
pub enum MockResponse<O, E> {
    /// A successful modeled response.
    Output(O),
    /// A modeled error.
    Error(E),
    /// An HTTP response.
    Http(HttpResponse),
}

impl<O, E> MockResponse<O, E> {
    /// Creates a new output response.
    pub fn output(output: O) -> Self {
        MockResponse::Output(output)
    }

    /// Creates a new error response.
    pub fn error(error: E) -> Self {
        MockResponse::Error(error)
    }

    /// Creates a new HTTP response.
    pub fn http(response: HttpResponse) -> Self {
        MockResponse::Http(response)
    }

    /// Creates a new HTTP response with the given status code and an empty body.
    pub fn status(status: u16) -> Self {
        let response = HttpResponse::new(StatusCode::try_from(status).unwrap(), SdkBody::empty());
        MockResponse::Http(response)
    }

    /// Creates a new HTTP response with status code 200 (OK) and an empty body.
    pub fn ok() -> Self {
        Self::status(200)
    }

    /// Creates a new HTTP response with status code 404 (Not Found) and an empty body.
    pub fn not_found() -> Self {
        Self::status(404)
    }

    /// Creates a new HTTP response with status code 500 (Internal Server Error) and an empty body.
    pub fn server_error() -> Self {
        Self::status(500)
    }

    /// Creates a new HTTP response with status code 503 (Service Unavailable) and an empty body.
    pub fn service_unavailable() -> Self {
        Self::status(503)
    }

    /// Creates a new HTTP response with status code 429 (Too Many Requests) and an empty body.
    pub fn throttled() -> Self {
        Self::status(429)
    }
}

/// A macro for creating a MockResponse from various types.
///
/// This macro handles the following cases:
/// - `mock_response!(output)` -> `MockResponse::Output(output)`
/// - `mock_response!(error: error)` -> `MockResponse::Error(error)`
/// - `mock_response!(http: http_response)` -> `MockResponse::Http(http_response)`
/// - `mock_response!(status: status)` -> `MockResponse::status(status)`
/// - `mock_response!(status: status, body: body)` -> `MockResponse::Http(HttpResponse::new(status, body))`
#[macro_export]
macro_rules! mock_response {
    // Output (default case)
    ($output:expr) => {
        $crate::MockResponse::Output($output)
    };

    // Error
    (error: $error:expr) => {
        $crate::MockResponse::Error($error)
    };

    // HTTP response
    (http: $response:expr) => {
        $crate::MockResponse::Http($response)
    };

    // Status code
    (status: $status:expr) => {
        $crate::MockResponse::status($status)
    };

    // Status code with body
    (status: $status:expr, body: $body:expr) => {{
        let response =
            HttpResponse::new(StatusCode::try_from($status).unwrap(), SdkBody::from($body));
        $crate::MockResponse::Http(response)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as StdError;
    use std::fmt;

    // Simple test types
    #[derive(Debug, Clone, PartialEq)]
    struct TestOutput {
        content: String,
    }

    impl TestOutput {
        fn new(content: &str) -> Self {
            Self {
                content: content.to_string(),
            }
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct TestError {
        message: String,
    }

    impl TestError {
        fn new(message: &str) -> Self {
            Self {
                message: message.to_string(),
            }
        }
    }

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.message)
        }
    }

    impl StdError for TestError {}

    #[test]
    fn test_mock_response_macro() {
        // Test output case
        let output = TestOutput::new("test");
        let response: MockResponse<TestOutput, TestError> = mock_response!(output.clone());
        match response {
            MockResponse::Output(o) => assert_eq!(o, output),
            _ => panic!("Expected Output variant"),
        }

        // Test error case
        let error = TestError::new("error");
        let response: MockResponse<TestOutput, TestError> = mock_response!(error: error.clone());
        match response {
            MockResponse::Error(e) => assert_eq!(e, error),
            _ => panic!("Expected Error variant"),
        }

        // Test http case
        let http_response =
            HttpResponse::new(StatusCode::try_from(200).unwrap(), SdkBody::from("body"));
        let response: MockResponse<TestOutput, TestError> = mock_response!(http: http_response);
        match response {
            MockResponse::Http(h) => {
                assert_eq!(h.status(), StatusCode::try_from(200).unwrap());
                assert_eq!(h.body().bytes().unwrap(), b"body")
            }
            _ => panic!("Expected Http variant"),
        }

        // Test status case
        let response: MockResponse<TestOutput, TestError> = mock_response!(status: 418);
        match response {
            MockResponse::Http(h) => {
                assert_eq!(h.status(), StatusCode::try_from(418).unwrap());
                assert!(h.body().bytes().unwrap().is_empty());
            }
            _ => panic!("Expected Http variant"),
        }

        // Test status with body case
        let response: MockResponse<TestOutput, TestError> =
            mock_response!(status: 418, body: "I'm a teapot");
        match response {
            MockResponse::Http(h) => {
                assert_eq!(h.status(), StatusCode::try_from(418).unwrap());
                assert_eq!(h.body().bytes().unwrap(), b"I'm a teapot");
            }
            _ => panic!("Expected Http variant"),
        }
    }
}
