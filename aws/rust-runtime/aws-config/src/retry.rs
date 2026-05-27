/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Retry configuration

// Re-export from aws-smithy-types
pub use aws_smithy_types::retry::ErrorKind;
pub use aws_smithy_types::retry::ProvideErrorKind;
pub use aws_smithy_types::retry::RetryConfig;
pub use aws_smithy_types::retry::RetryConfigBuilder;
pub use aws_smithy_types::retry::RetryKind;
pub use aws_smithy_types::retry::RetryMode;

use aws_credential_types::provider::error::CredentialsError;
use aws_runtime::retries::classifiers::{THROTTLING_ERRORS, TRANSIENT_ERRORS};
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_types::error::metadata::ProvideErrorMetadata;

/// Classify an `SdkError` from an inner SDK call (e.g. STS, SSO) into the appropriate
/// `CredentialsError` variant for operation-level retry.
///
/// - `TimeoutError` → `CredentialsError::transient_error`
/// - `DispatchFailure` with timeout/IO/other → `CredentialsError::transient_error`
/// - `ServiceError` with a throttling or transient error code → `CredentialsError::transient_error`
/// - All other errors → `CredentialsError::provider_error`
pub(crate) fn classify_credentials_error<E, R>(sdk_error: SdkError<E, R>) -> CredentialsError
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
    R: std::fmt::Debug + Send + Sync + 'static,
{
    match &sdk_error {
        SdkError::TimeoutError(_) => CredentialsError::transient_error(sdk_error),
        SdkError::DispatchFailure(df) => {
            if df.is_timeout() || df.is_io() || df.as_other().is_some() {
                CredentialsError::transient_error(sdk_error)
            } else {
                CredentialsError::provider_error(sdk_error)
            }
        }
        SdkError::ServiceError(ctx) => {
            let code = ctx.err().code().unwrap_or("");
            if THROTTLING_ERRORS.contains(&code) || TRANSIENT_ERRORS.contains(&code) {
                CredentialsError::transient_error(sdk_error)
            } else {
                CredentialsError::provider_error(sdk_error)
            }
        }
        _ => CredentialsError::provider_error(sdk_error),
    }
}

/// Errors for retry configuration
pub mod error {
    use std::fmt;
    use std::num::ParseIntError;

    // Re-export from aws-smithy-types
    pub use aws_smithy_types::retry::RetryModeParseError;

    #[derive(Debug)]
    pub(crate) enum RetryConfigErrorKind {
        /// The configured retry mode wasn't recognized.
        InvalidRetryMode {
            /// Cause of the error.
            source: RetryModeParseError,
        },
        /// Max attempts must be greater than zero.
        MaxAttemptsMustNotBeZero,
        /// The max attempts value couldn't be parsed to an integer.
        FailedToParseMaxAttempts {
            /// Cause of the error.
            source: ParseIntError,
        },
    }

    /// Failure to parse retry config from profile file or environment variable.
    #[derive(Debug)]
    pub struct RetryConfigError {
        pub(crate) kind: RetryConfigErrorKind,
    }

    impl fmt::Display for RetryConfigError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            use RetryConfigErrorKind::*;
            match &self.kind {
                InvalidRetryMode { .. } => {
                    write!(f, "invalid retry configuration")
                }
                MaxAttemptsMustNotBeZero => {
                    write!(f, "invalid configuration: It is invalid to set max attempts to 0. Unset it or set it to an integer greater than or equal to one.")
                }
                FailedToParseMaxAttempts { .. } => {
                    write!(f, "failed to parse max attempts",)
                }
            }
        }
    }

    impl std::error::Error for RetryConfigError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            use RetryConfigErrorKind::*;
            match &self.kind {
                InvalidRetryMode { source, .. } => Some(source),
                FailedToParseMaxAttempts { source, .. } => Some(source),
                MaxAttemptsMustNotBeZero => None,
            }
        }
    }

    impl From<RetryConfigErrorKind> for RetryConfigError {
        fn from(kind: RetryConfigErrorKind) -> Self {
            Self { kind }
        }
    }
}
