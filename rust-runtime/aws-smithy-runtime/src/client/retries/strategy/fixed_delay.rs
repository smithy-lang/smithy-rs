/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::client::retries::classifiers::run_classifiers_on_ctx;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::InterceptorContext;
use aws_smithy_runtime_api::client::retries::classifiers::RetryAction;
use aws_smithy_runtime_api::client::retries::{RequestAttempts, RetryStrategy, ShouldAttempt};
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::ConfigBag;
use std::time::Duration;
use tracing::debug;

/// A retry policy used in tests. This relies on an error classifier already present in the config bag.
/// If a server response is retryable, it will be retried after a fixed delay.
#[derive(Debug, Clone)]
pub struct FixedDelayRetryStrategy {
    fixed_delay: Duration,
    max_attempts: u32,
}

impl FixedDelayRetryStrategy {
    /// Create a new retry policy with a fixed delay.
    pub fn new(fixed_delay: Duration) -> Self {
        Self {
            fixed_delay,
            max_attempts: 4,
        }
    }

    /// Create a new retry policy with a one second delay.
    pub fn one_second_delay() -> Self {
        Self::new(Duration::from_secs(1))
    }
}

impl RetryStrategy for FixedDelayRetryStrategy {
    fn should_attempt_initial_request(
        &self,
        _runtime_components: &RuntimeComponents,
        _cfg: &ConfigBag,
    ) -> Result<ShouldAttempt, BoxError> {
        Ok(ShouldAttempt::Yes)
    }

    fn should_attempt_retry(
        &self,
        ctx: &InterceptorContext,
        runtime_components: &RuntimeComponents,
        cfg: &ConfigBag,
    ) -> Result<ShouldAttempt, BoxError> {
        // Look a the result. If it's OK then we're done; No retry required. Otherwise, we need to inspect it
        let output_or_error = ctx.output_or_error().expect(
            "This must never be called without reaching the point where the result exists.",
        );
        if output_or_error.is_ok() {
            tracing::trace!("request succeeded, no retry necessary");
            return Ok(ShouldAttempt::No);
        }

        // Check if we're out of attempts
        let request_attempts = cfg
            .load::<RequestAttempts>()
            .expect("at least one request attempt is made before any retry is attempted");
        if request_attempts.attempts() >= self.max_attempts {
            tracing::trace!(
                attempts = request_attempts.attempts(),
                max_attempts = self.max_attempts,
                "not retrying because we are out of attempts"
            );
            return Ok(ShouldAttempt::No);
        }

        let retry_classifiers = runtime_components.retry_classifiers();
        let classifier_result = run_classifiers_on_ctx(retry_classifiers, ctx);

        let backoff = match classifier_result {
            RetryAction::Explicit(_) => self.fixed_delay,
            RetryAction::Error(_) => self.fixed_delay,
            RetryAction::DontRetry => {
                debug!(
                    attempts = request_attempts.attempts(),
                    max_attempts = self.max_attempts,
                    "encountered unretryable error"
                );
                return Ok(ShouldAttempt::No);
            }
            _ => {
                unreachable!("RetryAction is non-exhaustive. Therefore, we need to cover this unreachable case.")
            }
        };

        debug!(
            "attempt {} failed with {:?}; retrying after {:?}",
            request_attempts.attempts(),
            classifier_result,
            backoff
        );

        Ok(ShouldAttempt::YesAfterDelay(backoff))
    }
}
