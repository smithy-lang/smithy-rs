pub mod user_agent;
use smithy_http::retry::ClassifyResponse;
use smithy_types::retry::{ErrorKind, ProvideErrorKind, RetryKind};
use std::time::Duration;

/// A retry policy that models AWS error codes as outlined in the SEP
///
/// In order of priority:
/// 1. The `x-amz-retry-after` header is checked
/// 2. The modeled error retry mode is checked
/// 3. The code is checked against a predetermined list of throttling errors & transient error codes
/// 4. The status code is checked against a predetermined list of status codes
#[non_exhaustive]
pub struct AwsErrorRetryPolicy;

const TRANSIENT_ERROR_STATUS_CODES: [u16; 2] = [400, 408];
const THROTTLING_ERRORS: &[&str] = &[
    "Throttling",
    "ThrottlingException",
    "ThrottledException",
    "RequestThrottledException",
    "TooManyRequestsException",
    "ProvisionedThroughputExceededException",
    "TransactionInProgressException",
    "RequestLimitExceeded",
    "BandwidthLimitExceeded",
    "LimitExceededException",
    "RequestThrottled",
    "SlowDown",
    "PriorRequestNotComplete",
    "EC2ThrottledException",
];
const TRANSIENT_ERRORS: &[&str] = &["RequestTimeout", "RequestTimeoutException"];

impl AwsErrorRetryPolicy {
    /// Create an `AwsErrorRetryPolicy` with the default set of known error & status codes
    pub fn new() -> Self {
        AwsErrorRetryPolicy
    }
}

impl Default for AwsErrorRetryPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl ClassifyResponse for AwsErrorRetryPolicy {
    fn classify<E, B>(&self, err: E, response: &http::Response<B>) -> RetryKind
    where
        E: ProvideErrorKind,
    {
        if let Some(retry_after_delay) = response
            .headers()
            .get("x-amz-retry-after")
            .and_then(|header| header.to_str().ok())
            .and_then(|header| header.parse::<u64>().ok())
        {
            return RetryKind::Explicit(Duration::from_millis(retry_after_delay));
        }
        if let Some(kind) = err.retryable_error_kind() {
            return RetryKind::Error(kind);
        };
        if let Some(code) = err.code() {
            if THROTTLING_ERRORS.contains(&code) {
                return RetryKind::Error(ErrorKind::ThrottlingError);
            }
            if TRANSIENT_ERRORS.contains(&code) {
                return RetryKind::Error(ErrorKind::TransientError);
            }
        };
        if TRANSIENT_ERROR_STATUS_CODES.contains(&response.status().as_u16()) {
            return RetryKind::Error(ErrorKind::TransientError);
        };
        // TODO: is IDPCommunicationError modeled yet?
        RetryKind::NotRetryable
    }
}

#[cfg(test)]
mod test {
    use crate::AwsErrorRetryPolicy;
    use smithy_http::retry::ClassifyResponse;
    use smithy_types::retry::{ErrorKind, ProvideErrorKind, RetryKind};
    use std::time::Duration;

    struct UnmodeledError;

    struct CodedError {
        code: &'static str,
    }

    impl ProvideErrorKind for UnmodeledError {
        fn retryable_error_kind(&self) -> Option<ErrorKind> {
            None
        }

        fn code(&self) -> Option<&str> {
            None
        }
    }

    impl ProvideErrorKind for CodedError {
        fn retryable_error_kind(&self) -> Option<ErrorKind> {
            None
        }

        fn code(&self) -> Option<&str> {
            Some(self.code)
        }
    }

    #[test]
    fn not_an_error() {
        let policy = AwsErrorRetryPolicy::new();
        let test_response = http::Response::new("OK");
        assert_eq!(
            policy.classify(UnmodeledError, &test_response),
            RetryKind::NotRetryable
        );
    }

    #[test]
    fn classify_by_response_status() {
        let policy = AwsErrorRetryPolicy::new();
        let test_resp = http::Response::builder()
            .status(408)
            .body("error!")
            .unwrap();
        assert_eq!(
            policy.classify(UnmodeledError, &test_resp),
            RetryKind::Error(ErrorKind::TransientError)
        );
    }

    #[test]
    fn classify_by_error_code() {
        let test_response = http::Response::new("OK");
        let policy = AwsErrorRetryPolicy::new();

        assert_eq!(
            policy.classify(CodedError { code: "Throttling" }, &test_response),
            RetryKind::Error(ErrorKind::ThrottlingError)
        );

        assert_eq!(
            policy.classify(
                CodedError {
                    code: "RequestTimeout"
                },
                &test_response,
            ),
            RetryKind::Error(ErrorKind::TransientError)
        )
    }

    #[test]
    fn classify_generic() {
        let err = smithy_types::Error {
            code: Some("SlowDown".to_string()),
            message: None,
            request_id: None,
        };
        let test_response = http::Response::new("OK");
        let policy = AwsErrorRetryPolicy::new();
        assert_eq!(
            policy.classify(err, &test_response),
            RetryKind::Error(ErrorKind::ThrottlingError)
        );
    }

    #[test]
    fn classify_by_error_kind() {
        struct ModeledRetries;
        let test_response = http::Response::new("OK");
        impl ProvideErrorKind for ModeledRetries {
            fn retryable_error_kind(&self) -> Option<ErrorKind> {
                Some(ErrorKind::ClientError)
            }

            fn code(&self) -> Option<&str> {
                // code should not be called when `error_kind` is provided
                unimplemented!()
            }
        }

        let policy = AwsErrorRetryPolicy::new();

        assert_eq!(
            policy.classify(ModeledRetries, &test_response),
            RetryKind::Error(ErrorKind::ClientError)
        );
    }

    #[test]
    fn test_retry_after_header() {
        let policy = AwsErrorRetryPolicy::new();
        let test_response = http::Response::builder()
            .header("x-amz-retry-after", "5000")
            .body("retry later")
            .unwrap();

        assert_eq!(
            policy.classify(UnmodeledError, &test_response),
            RetryKind::Explicit(Duration::from_millis(5000))
        );
    }
}
