use http::StatusCode;
use smithy_http::retry::HttpRetryPolicy;
use smithy_types::retry::{ErrorKind, ProvideErrorKind, RetryKind};
use std::collections::HashSet;
use std::iter::FromIterator;
use std::time::Duration;

/// A retry policy that models AWS error codes as outlined in the SEP
///
/// In order of priority:
/// 1. The `x-amz-retry-after` header is checked
/// 2. The modeled error retry mode is checked
/// 3. The code is checked against a predetermined list of throttling errors & transient error codes
/// 4. The status code is checked against a predetermined list of status codes
pub struct AwsErrorRetryPolicy {
    throttling_errors: HashSet<&'static str>,
    transient_errors: HashSet<&'static str>,
    transient_status_codes: HashSet<StatusCode>,
}

impl AwsErrorRetryPolicy {
    /// Create an `AwsErrorRetryPolicy` with the default set of known error & status codes
    pub fn new() -> Self {
        let throttling_errors = &[
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

        let transient_errors = ["RequestTimeout", "RequestTimeoutException"];
        let throttling_errors = HashSet::from_iter(throttling_errors.into_iter().cloned());
        let transient_errors = HashSet::from_iter(transient_errors.iter().cloned());
        let transient_status_codes = &[
            StatusCode::from_u16(400).unwrap(),
            StatusCode::from_u16(408).unwrap(),
        ];
        let transient_status_codes =
            HashSet::from_iter(transient_status_codes.into_iter().cloned());
        AwsErrorRetryPolicy {
            throttling_errors,
            transient_errors,
            transient_status_codes,
        }
    }
}

impl HttpRetryPolicy for AwsErrorRetryPolicy {
    fn retry_kind<E, B>(&self, err: E, response: &http::Response<B>) -> RetryKind
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
        if let Some(kind) = err.error_kind() {
            return RetryKind::Error(kind);
        };
        if let Some(code) = err.code() {
            if self.throttling_errors.contains(code) {
                return RetryKind::Error(ErrorKind::ThrottlingError);
            }
            if self.transient_errors.contains(code) {
                return RetryKind::Error(ErrorKind::TransientError);
            }
        };
        if self.transient_status_codes.contains(&response.status()) {
            return RetryKind::Error(ErrorKind::TransientError);
        };
        // TODO: is IDPCommunicationError modeled yet?
        RetryKind::NotRetryable
    }
}

#[cfg(test)]
mod test {
    use crate::AwsErrorRetryPolicy;

    #[test]
    fn smoke_test() {
        let policy = AwsErrorRetryPolicy::new();
    }
}
