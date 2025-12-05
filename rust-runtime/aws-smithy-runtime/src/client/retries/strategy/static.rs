/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Static retry strategy implementation with token bucket rate limiting.
//!
//! This module provides a retry strategy that uses a token bucket for rate limiting
//! combined with exponential backoff, but without dynamic rate calculation.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::OwnedSemaphorePermit;
use tracing::debug;

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::InterceptorContext;
use aws_smithy_runtime_api::client::retries::classifiers::{RetryAction, RetryReason};
use aws_smithy_runtime_api::client::retries::{RequestAttempts, RetryStrategy, ShouldAttempt};
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
use aws_smithy_types::retry::RetryConfig;

use crate::client::retries::classifiers::run_classifiers_on_ctx;
use crate::client::retries::strategy::standard::calculate_exponential_backoff;
use crate::client::retries::strategy::standard::get_seconds_since_unix_epoch;
use crate::client::retries::token_bucket::TokenBucket;

/// Retry strategy with static rate limiting and exponential backoff.
///
/// This strategy combines token bucket rate limiting with exponential backoff,
/// using fixed token bucket parameters (no dynamic rate calculation like adaptive retry).
///
/// ## Key Features
/// - Token bucket for rate limiting (must have tokens to retry)
/// - Exponential backoff with jitter for retry timing
/// - Shares infrastructure with `StandardRetryStrategy` (TokenBucketProvider, backoff calculation)
/// - Token costs configured via `TokenBucket` (use `TokenBucket::builder()` to customize)
///
/// ## Default Behavior
/// - **Time-based refill**: Disabled (0.0 tokens/sec) - truly static behavior
/// - **Success awards**: 1.0 token per successful request
/// - **Token costs**: Standard costs (5 tokens per retry, 10 for timeouts)
///
/// ## Differences from Standard Retry
/// - No dynamic rate limiting (ClientRateLimiter)
/// - Token bucket and its costs configured externally via `TokenBucket::builder()`
/// - Optional time-based refill (disabled by default for static behavior)

#[derive(Debug)]
pub struct StaticRetryStrategy {
    retry_permit: Mutex<Option<OwnedSemaphorePermit>>,
    refill_state: Arc<Mutex<RefillState>>,
    refill_rate: f64,
    success_award: f64,
}

/// Internal state for tracking token bucket refill.
#[derive(Debug, Default)]
struct RefillState {
    /// Accumulates fractional tokens from both time-based and success-based awards.
    fractional_tokens: f64,
    /// Last timestamp when refill calculation occurred (seconds since UNIX_EPOCH).
    last_refill_time: Option<f64>,
}

impl Storable for StaticRetryStrategy {
    type Storer = StoreReplace<Self>;
}

impl Default for StaticRetryStrategy {
    fn default() -> Self {
        Self {
            retry_permit: Mutex::new(None),
            refill_state: Arc::new(Mutex::new(RefillState {
                fractional_tokens: 0.0,
                last_refill_time: None,
            })),
            refill_rate: 0.0,   // no time-based refill by default (static behavior)
            success_award: 1.0, // default to 1 token per success
        }
    }
}

/// Builder for constructing a `StaticRetryStrategy` with custom configuration.
///
/// Note: Token costs are configured via `TokenBucket::builder()`, not here.
#[derive(Debug, Default)]
pub struct StaticRetryStrategyBuilder {
    refill_rate: Option<f64>,
    success_award: Option<f64>,
}

impl StaticRetryStrategyBuilder {
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the token bucket refill rate (tokens per second).
    ///
    /// When set to a positive value, the token bucket will continuously refill
    /// at this rate based on elapsed time. A value of 0.0 disables time-based refill.
    pub fn refill_rate(mut self, rate: f64) -> Self {
        self.refill_rate = Some(rate);
        self
    }

    /// Sets the number of tokens awarded on successful requests.
    ///
    /// After a successful request, this many tokens will be added back to the bucket.
    /// Defaults to 1.0 if not specified.
    pub fn success_award(mut self, award: f64) -> Self {
        self.success_award = Some(award);
        self
    }

    /// Builds a `StaticRetryStrategy` with the configured parameters.
    pub fn build(self) -> StaticRetryStrategy {
        StaticRetryStrategy {
            retry_permit: Mutex::new(None),
            refill_state: Arc::new(Mutex::new(RefillState {
                fractional_tokens: 0.0,
                last_refill_time: None,
            })),
            refill_rate: self.refill_rate.unwrap_or(0.0),
            success_award: self.success_award.unwrap_or(1.0),
        }
    }
}

impl StaticRetryStrategy {
    /// Creates a new `StaticRetryStrategy` with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a builder for constructing a `StaticRetryStrategy`.
    pub fn builder() -> StaticRetryStrategyBuilder {
        StaticRetryStrategyBuilder::default()
    }

    /// Releases the currently held retry permit, if any.
    ///
    /// Returns whether a permit was actually released.
    fn release_retry_permit(&self) -> ReleaseResult {
        let mut retry_permit = self.retry_permit.lock().unwrap();
        match retry_permit.take() {
            Some(p) => {
                drop(p);
                ReleaseResult::APermitWasReleased
            }
            None => ReleaseResult::NoPermitWasReleased,
        }
    }

    /// Sets a new retry permit, forgetting any previously held permit.
    ///
    /// If a previous permit exists, it is forgotten (removed from bucket permanently)
    /// to prevent double-counting when replacing permits.
    fn set_retry_permit(&self, new_retry_permit: OwnedSemaphorePermit) {
        let mut old_retry_permit = self.retry_permit.lock().unwrap();
        if let Some(p) = old_retry_permit.replace(new_retry_permit) {
            // CRITICAL: We must "forget" the old permit instead of dropping it.
            //
            // When a retry attempt is made, tokens are "spent" from the bucket and should
            // not be returned. If we drop() the permit, those tokens would go back to the
            // bucket, incorrectly making them available for future retries.
            p.forget()
        }
    }

    /// Calculates backoff duration for the next retry attempt.
    ///
    /// Uses exponential backoff with optional jitter, respecting server-requested
    /// delays when provided.
    fn calculate_backoff(
        &self,
        cfg: &ConfigBag,
        retry_cfg: &RetryConfig,
        retry_reason: &RetryAction,
    ) -> Result<Duration, ShouldAttempt> {
        let request_attempts = cfg
            .load::<RequestAttempts>()
            .expect("at least one request attempt is made before any retry is attempted")
            .attempts();

        match retry_reason {
            RetryAction::RetryIndicated(RetryReason::RetryableError { retry_after, .. }) => {
                if let Some(delay) = *retry_after {
                    let delay = delay.min(retry_cfg.max_backoff());
                    debug!("explicit request from server to delay {delay:?} before retrying");
                    Ok(delay)
                } else {
                    let base = if retry_cfg.use_static_exponential_base() {
                        1.0
                    } else {
                        fastrand::f64()
                    };
                    // Use shared backoff calculation from standard strategy
                    Ok(calculate_exponential_backoff(
                        base,
                        retry_cfg.initial_backoff().as_secs_f64(),
                        request_attempts - 1,
                        retry_cfg.max_backoff(),
                    ))
                }
            }
            RetryAction::RetryForbidden | RetryAction::NoActionIndicated => {
                debug!(
                    attempts = request_attempts,
                    max_attempts = retry_cfg.max_attempts(),
                    "encountered un-retryable error"
                );
                Err(ShouldAttempt::No)
            }
            _ => unreachable!("RetryAction is non-exhaustive"),
        }
    }

    /// Refills the token bucket based on elapsed time and refill rate.
    ///
    /// Adds whole tokens accumulated from fractional calculations back to the bucket.
    fn refill(&self, token_bucket: &TokenBucket, seconds_since_unix_epoch: f64) {
        // Pre-calculate all values outside the lock to minimize contention
        let (tokens_to_add, new_fractional, new_last_time) = {
            let state = self.refill_state.lock().unwrap();

            // Do calculations with current values
            let mut fractional = state.fractional_tokens;
            let mut last_time = state.last_refill_time;

            // Time-based refill (if enabled)
            if self.refill_rate > 0.0 {
                if let Some(lt) = last_time {
                    let elapsed = seconds_since_unix_epoch - lt;
                    fractional += elapsed * self.refill_rate;
                }
                last_time = Some(seconds_since_unix_epoch);
            }

            let whole_tokens = fractional.floor() as usize;
            let new_fractional = fractional - whole_tokens as f64;

            (whole_tokens, new_fractional, last_time)
        }; // Lock released here

        // Add tokens to bucket outside of lock
        if tokens_to_add > 0 {
            token_bucket.add_permits(tokens_to_add);
        }

        // Quick update with pre-calculated values
        {
            let mut state = self.refill_state.lock().unwrap();
            state.fractional_tokens = new_fractional;
            state.last_refill_time = new_last_time;
        } // Lock released immediately
    }
}

/// Result of attempting to release a retry permit.
enum ReleaseResult {
    /// A permit was successfully released back to the bucket.
    APermitWasReleased,
    /// No permit was held, so nothing was released.
    NoPermitWasReleased,
}

impl RetryStrategy for StaticRetryStrategy {
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
        let retry_cfg = cfg.load::<RetryConfig>().expect("retry config is required");
        let token_bucket = cfg.load::<TokenBucket>().expect("token bucket is required");

        let seconds_since_unix_epoch = get_seconds_since_unix_epoch(runtime_components);
        self.refill(token_bucket, seconds_since_unix_epoch);

        let retry_classifiers = runtime_components.retry_classifiers();
        let classifier_result = run_classifiers_on_ctx(retry_classifiers, ctx);

        // On success, award tokens
        if !ctx.is_failed() {
            if let ReleaseResult::NoPermitWasReleased = self.release_retry_permit() {
                let mut state = self.refill_state.lock().unwrap();
                state.fractional_tokens += self.success_award;
            }
        }

        let request_attempts = cfg
            .load::<RequestAttempts>()
            .expect("at least one request attempt is made before any retry is attempted")
            .attempts();

        if !classifier_result.should_retry() {
            debug!(
                "attempt #{request_attempts} classified as {:?}, not retrying",
                classifier_result
            );
            return Ok(ShouldAttempt::No);
        }

        if request_attempts >= retry_cfg.max_attempts() {
            debug!(
                attempts = request_attempts,
                max_attempts = retry_cfg.max_attempts(),
                "not retrying because we are out of attempts"
            );
            return Ok(ShouldAttempt::No);
        }

        // Acquire tokens using the token bucket's configured costs
        // (same pattern as StandardRetryStrategy)
        let error_kind = match &classifier_result {
            RetryAction::RetryIndicated(RetryReason::RetryableError { kind, .. }) => *kind,
            _ => aws_smithy_types::retry::ErrorKind::ServerError,
        };

        match token_bucket.acquire(&error_kind) {
            Some(permit) => {
                self.set_retry_permit(permit);

                let backoff = match self.calculate_backoff(cfg, retry_cfg, &classifier_result) {
                    Ok(value) => value,
                    Err(value) => return Ok(value),
                };

                debug!(
                    "attempt #{request_attempts} failed with {error_kind:?}; acquired tokens, retrying after {:?}",
                    backoff
                );
                Ok(ShouldAttempt::YesAfterDelay(backoff))
            }
            None => {
                debug!(
                    "attempt #{request_attempts} failed with {error_kind:?}; insufficient tokens, not retrying"
                );
                Ok(ShouldAttempt::No)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use aws_smithy_async::time::SystemTimeSource;
    use aws_smithy_runtime_api::client::interceptors::context::{
        Input, InterceptorContext, Output,
    };
    use aws_smithy_runtime_api::client::orchestrator::OrchestratorError;
    use aws_smithy_runtime_api::client::retries::classifiers::SharedRetryClassifier;
    use aws_smithy_runtime_api::client::retries::{
        AlwaysRetry, RequestAttempts, RetryStrategy, ShouldAttempt,
    };
    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
    use aws_smithy_types::config_bag::{ConfigBag, Layer};
    use aws_smithy_types::retry::{ErrorKind, RetryConfig};

    use super::StaticRetryStrategy;
    use crate::client::retries::TokenBucket;

    #[test]
    fn no_retry_on_success() {
        let cfg = ConfigBag::of_layers(vec![{
            let mut layer = Layer::new("test");
            layer.store_put(RetryConfig::standard());
            layer.store_put(RequestAttempts::new(1));
            layer.store_put(TokenBucket::default());
            layer
        }]);
        let rc = RuntimeComponentsBuilder::for_tests()
            .with_time_source(Some(SystemTimeSource::new()))
            .build()
            .unwrap();
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        let strategy = StaticRetryStrategy::default();
        ctx.set_output_or_error(Ok(Output::doesnt_matter()));

        let actual = strategy
            .should_attempt_retry(&ctx, &rc, &cfg)
            .expect("method is infallible for this use");
        assert_eq!(ShouldAttempt::No, actual);
    }

    #[test]
    fn retry_with_backoff() {
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::other("test error")));

        let rc = RuntimeComponentsBuilder::for_tests()
            .with_retry_classifier(SharedRetryClassifier::new(AlwaysRetry(
                ErrorKind::ServerError,
            )))
            .with_time_source(Some(SystemTimeSource::new()))
            .build()
            .unwrap();

        let mut layer = Layer::new("test");
        layer.store_put(RequestAttempts::new(1));
        layer.store_put(
            RetryConfig::standard()
                .with_use_static_exponential_base(true)
                .with_max_attempts(5),
        );
        layer.store_put(TokenBucket::default());
        let cfg = ConfigBag::of_layers(vec![layer]);

        let strategy = StaticRetryStrategy::new();
        let result = strategy.should_attempt_retry(&ctx, &rc, &cfg).unwrap();

        assert_eq!(result, ShouldAttempt::YesAfterDelay(Duration::from_secs(1)));
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn backoff_increases_exponentially() {
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::other("test error")));

        let rc = RuntimeComponentsBuilder::for_tests()
            .with_retry_classifier(SharedRetryClassifier::new(AlwaysRetry(
                ErrorKind::ServerError,
            )))
            .build()
            .unwrap();

        let strategy = StaticRetryStrategy::new();

        for (attempt, expected_seconds) in [(1, 1), (2, 2), (3, 4), (4, 8)] {
            let mut layer = Layer::new("test");
            layer.store_put(RequestAttempts::new(attempt));
            layer.store_put(
                RetryConfig::standard()
                    .with_use_static_exponential_base(true)
                    .with_max_attempts(10),
            );
            layer.store_put(TokenBucket::default());
            let cfg = ConfigBag::of_layers(vec![layer]);

            let result = strategy.should_attempt_retry(&ctx, &rc, &cfg).unwrap();
            assert_eq!(
                result,
                ShouldAttempt::YesAfterDelay(Duration::from_secs(expected_seconds))
            );
        }
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn no_retry_when_no_tokens() {
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::other("test error")));

        let rc = RuntimeComponentsBuilder::for_tests()
            .with_retry_classifier(SharedRetryClassifier::new(AlwaysRetry(
                ErrorKind::ServerError,
            )))
            .build()
            .unwrap();

        let mut layer = Layer::new("test");
        layer.store_put(RequestAttempts::new(1));
        layer.store_put(RetryConfig::standard().with_max_attempts(5));
        layer.store_put(TokenBucket::new(5));
        let cfg = ConfigBag::of_layers(vec![layer]);
        let token_bucket = cfg.load::<TokenBucket>().unwrap().clone();

        let strategy = StaticRetryStrategy::new();

        let result = strategy.should_attempt_retry(&ctx, &rc, &cfg).unwrap();
        assert!(matches!(result, ShouldAttempt::YesAfterDelay(_)));
        assert_eq!(token_bucket.available_permits(), 0);

        let mut layer2 = Layer::new("test");
        layer2.store_put(RequestAttempts::new(2));
        let cfg2 = ConfigBag::of_layers(vec![layer, layer2]);
        let result2 = strategy.should_attempt_retry(&ctx, &rc, &cfg2).unwrap();
        assert_eq!(result2, ShouldAttempt::No);
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn respects_max_attempts() {
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::other("test error")));

        let rc = RuntimeComponentsBuilder::for_tests()
            .with_retry_classifier(SharedRetryClassifier::new(AlwaysRetry(
                ErrorKind::ServerError,
            )))
            .build()
            .unwrap();

        let mut layer = Layer::new("test");
        layer.store_put(RequestAttempts::new(3));
        layer.store_put(RetryConfig::standard().with_max_attempts(3));
        layer.store_put(TokenBucket::default());
        let cfg = ConfigBag::of_layers(vec![layer]);

        let strategy = StaticRetryStrategy::new();
        let result = strategy.should_attempt_retry(&ctx, &rc, &cfg).unwrap();

        assert_eq!(result, ShouldAttempt::No);
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn token_award_on_success() {
        // Tests success award - UNIQUE to StaticRetryStrategy behavior
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::other("test error")));

        let rc = RuntimeComponentsBuilder::for_tests()
            .with_retry_classifier(SharedRetryClassifier::new(AlwaysRetry(
                ErrorKind::ServerError,
            )))
            .build()
            .unwrap();

        let mut layer = Layer::new("test");
        layer.store_put(RequestAttempts::new(1));
        layer.store_put(RetryConfig::standard().with_max_attempts(5));
        layer.store_put(TokenBucket::new(100));
        let cfg = ConfigBag::of_layers(vec![layer]);
        let token_bucket = cfg.load::<TokenBucket>().unwrap().clone();

        let strategy = StaticRetryStrategy::new();

        // First retry consumes tokens
        let result = strategy.should_attempt_retry(&ctx, &rc, &cfg).unwrap();
        assert!(matches!(result, ShouldAttempt::YesAfterDelay(_)));
        assert_eq!(token_bucket.available_permits(), 95); // 100 - 5

        // Success should award tokens
        ctx.set_output_or_error(Ok(Output::doesnt_matter()));
        let mut layer2 = Layer::new("test");
        layer2.store_put(RequestAttempts::new(2));
        let cfg2 = ConfigBag::of_layers(vec![layer, layer2]);

        let result2 = strategy.should_attempt_retry(&ctx, &rc, &cfg2).unwrap();
        assert_eq!(result2, ShouldAttempt::No); // Success = no retry
        assert_eq!(token_bucket.available_permits(), 96); // 95 + 1 (default award)
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn time_based_refill() {
        use aws_smithy_async::time::StaticTimeSource;
        use std::time::SystemTime;

        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::other("test error")));

        // Strategy with 10 tokens per second refill rate, no success award
        let strategy = StaticRetryStrategy::builder()
            .refill_rate(10.0)
            .success_award(0.0) // Disable success award to test only time-based refill
            .build();

        // Start at time T=0
        let time_source = StaticTimeSource::new(SystemTime::UNIX_EPOCH);
        let rc = RuntimeComponentsBuilder::for_tests()
            .with_retry_classifier(SharedRetryClassifier::new(AlwaysRetry(
                ErrorKind::ServerError,
            )))
            .with_time_source(Some(time_source.clone()))
            .build()
            .unwrap();

        let mut layer = Layer::new("test");
        layer.store_put(RequestAttempts::new(1));
        layer.store_put(RetryConfig::standard().with_max_attempts(5));
        layer.store_put(TokenBucket::new(10));
        let cfg = ConfigBag::of_layers(vec![layer]);
        let token_bucket = cfg.load::<TokenBucket>().unwrap().clone();

        // First attempt uses tokens
        let result = strategy.should_attempt_retry(&ctx, &rc, &cfg).unwrap();
        assert!(matches!(result, ShouldAttempt::YesAfterDelay(_)));
        assert_eq!(token_bucket.available_permits(), 5); // 10 - 5

        // Advance time by 1 second (should add 10 tokens)
        time_source.advance(Duration::from_secs(1));

        let mut layer2 = Layer::new("test");
        layer2.store_put(RequestAttempts::new(2));
        let cfg2 = ConfigBag::of_layers(vec![layer, layer2]);

        let result2 = strategy.should_attempt_retry(&ctx, &rc, &cfg2).unwrap();
        assert!(matches!(result2, ShouldAttempt::YesAfterDelay(_)));
        // 5 (remaining) + 10 (refilled) - 5 (consumed) = 10, but capped at bucket max of 10
        assert_eq!(token_bucket.available_permits(), 5); // 10 - 5 (second retry)
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn interaction_between_time_and_success_refill() {
        use aws_smithy_async::time::StaticTimeSource;
        use std::time::SystemTime;

        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::other("test error")));

        // Strategy with both time-based and success-based refill
        let strategy = StaticRetryStrategy::builder()
            .refill_rate(2.0) // 2 tokens per second
            .success_award(3.0) // 3 tokens per success
            .build();

        let time_source = StaticTimeSource::new(SystemTime::UNIX_EPOCH);
        let rc = RuntimeComponentsBuilder::for_tests()
            .with_retry_classifier(SharedRetryClassifier::new(AlwaysRetry(
                ErrorKind::ServerError,
            )))
            .with_time_source(Some(time_source.clone()))
            .build()
            .unwrap();

        let mut layer = Layer::new("test");
        layer.store_put(RequestAttempts::new(1));
        layer.store_put(RetryConfig::standard().with_max_attempts(5));
        layer.store_put(TokenBucket::new(50));
        let cfg = ConfigBag::of_layers(vec![layer]);
        let token_bucket = cfg.load::<TokenBucket>().unwrap().clone();

        // First retry consumes tokens
        let result = strategy.should_attempt_retry(&ctx, &rc, &cfg).unwrap();
        assert!(matches!(result, ShouldAttempt::YesAfterDelay(_)));
        assert_eq!(token_bucket.available_permits(), 45); // 50 - 5

        // Advance time by 2.5 seconds (should add 5 tokens from time)
        time_source.advance(Duration::from_millis(2500));

        // Simulate success (should add 3 tokens from success award)
        ctx.set_output_or_error(Ok(Output::doesnt_matter()));
        let mut layer2 = Layer::new("test");
        layer2.store_put(RequestAttempts::new(2));
        let cfg2 = ConfigBag::of_layers(vec![layer, layer2]);

        let result2 = strategy.should_attempt_retry(&ctx, &rc, &cfg2).unwrap();
        assert_eq!(result2, ShouldAttempt::No); // Success = no retry

        // Should have: 45 (remaining) + 5 (time refill) + 3 (success award) = 53,
        // but capped at bucket max of 50
        assert_eq!(token_bucket.available_permits(), 50);
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn fractional_token_accumulation() {
        use aws_smithy_async::time::StaticTimeSource;
        use std::time::SystemTime;

        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::other("test error")));

        // Strategy with fractional refill rate
        let strategy = StaticRetryStrategy::builder()
            .refill_rate(0.3) // 0.3 tokens per second (requires 3.33 seconds for 1 token)
            .success_award(0.7) // 0.7 fractional tokens per success
            .build();

        let time_source = StaticTimeSource::new(SystemTime::UNIX_EPOCH);
        let rc = RuntimeComponentsBuilder::for_tests()
            .with_retry_classifier(SharedRetryClassifier::new(AlwaysRetry(
                ErrorKind::ServerError,
            )))
            .with_time_source(Some(time_source.clone()))
            .build()
            .unwrap();

        let mut layer = Layer::new("test");
        layer.store_put(RequestAttempts::new(1));
        layer.store_put(RetryConfig::standard().with_max_attempts(10));
        layer.store_put(TokenBucket::new(10));
        let cfg = ConfigBag::of_layers(vec![layer]);
        let token_bucket = cfg.load::<TokenBucket>().unwrap().clone();

        // First retry - should work (has tokens)
        let result = strategy.should_attempt_retry(&ctx, &rc, &cfg).unwrap();
        assert!(matches!(result, ShouldAttempt::YesAfterDelay(_)));
        assert_eq!(token_bucket.available_permits(), 5); // 10 - 5

        // Advance by 2 seconds: should add 0.6 fractional tokens (not enough for 1 whole token)
        time_source.advance(Duration::from_secs(2));

        let mut layer2 = Layer::new("test");
        layer2.store_put(RequestAttempts::new(2));
        let cfg2 = ConfigBag::of_layers(vec![layer, layer2]);

        let result2 = strategy.should_attempt_retry(&ctx, &rc, &cfg2).unwrap();
        assert!(matches!(result2, ShouldAttempt::YesAfterDelay(_)));
        assert_eq!(token_bucket.available_permits(), 0); // 5 - 5, no fractional tokens added yet

        // Advance by 2 more seconds: total 1.2 fractional tokens (enough for 1 whole token)
        time_source.advance(Duration::from_secs(2));

        let mut layer3 = Layer::new("test");
        layer3.store_put(RequestAttempts::new(3));
        let cfg3 = ConfigBag::of_layers(vec![layer, layer2, layer3]);

        let result3 = strategy.should_attempt_retry(&ctx, &rc, &cfg3).unwrap();
        assert_eq!(result3, ShouldAttempt::No); // No tokens left after refill and consumption

        // Should have added 1 token from fractional accumulation (1.2 -> 1 + 0.2 remaining)
        assert_eq!(token_bucket.available_permits(), 0); // 0 + 1 (accumulated) - 5 = not enough
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn fractional_overflow_edge_cases() {
        use aws_smithy_async::time::StaticTimeSource;
        use std::time::SystemTime;

        let mut ctx = InterceptorContext::new(Input::doesnt_matter());

        // Strategy with large fractional awards that could accumulate significantly
        let strategy = StaticRetryStrategy::builder()
            .refill_rate(1000.0) // Very high refill rate
            .success_award(99.9) // Large fractional success award
            .build();

        let time_source = StaticTimeSource::new(SystemTime::UNIX_EPOCH);
        let rc = RuntimeComponentsBuilder::for_tests()
            .with_retry_classifier(SharedRetryClassifier::new(AlwaysRetry(
                ErrorKind::ServerError,
            )))
            .with_time_source(Some(time_source.clone()))
            .build()
            .unwrap();

        let mut layer = Layer::new("test");
        layer.store_put(RequestAttempts::new(1));
        layer.store_put(RetryConfig::standard().with_max_attempts(5));
        layer.store_put(TokenBucket::new(1000));
        let cfg = ConfigBag::of_layers(vec![layer]);
        let token_bucket = cfg.load::<TokenBucket>().unwrap().clone();

        // Simulate multiple successes to accumulate fractional tokens
        ctx.set_output_or_error(Ok(Output::doesnt_matter()));

        // First success adds 99.9 fractional tokens
        let result = strategy.should_attempt_retry(&ctx, &rc, &cfg).unwrap();
        assert_eq!(result, ShouldAttempt::No); // Success = no retry
        assert_eq!(token_bucket.available_permits(), 1000); // 1000 + 99 (floor of 99.9)

        // Advance time by 0.001 second to add 1 more token from time refill
        // This should push accumulated fractional tokens over the edge
        time_source.advance(Duration::from_millis(1));

        let mut layer2 = Layer::new("test");
        layer2.store_put(RequestAttempts::new(2));
        let cfg2 = ConfigBag::of_layers(vec![layer, layer2]);

        let result2 = strategy.should_attempt_retry(&ctx, &rc, &cfg2).unwrap();
        assert_eq!(result2, ShouldAttempt::No); // Success = no retry

        // Should be capped at bucket maximum despite huge fractional accumulation
        assert_eq!(token_bucket.available_permits(), 1000); // Capped at bucket max
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn custom_token_costs() {
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::other("test error")));

        let rc = RuntimeComponentsBuilder::for_tests()
            .with_retry_classifier(SharedRetryClassifier::new(AlwaysRetry(
                ErrorKind::TransientError,
            ))) // Use TransientError
            .with_time_source(Some(SystemTimeSource::new()))
            .build()
            .unwrap();

        // Custom token bucket with different costs
        let custom_bucket = TokenBucket::builder()
            .capacity(100)
            .retry_cost(10) // Normal retries cost 10 tokens
            .timeout_retry_cost(25) // Timeout retries cost 25 tokens
            .build();

        let mut layer = Layer::new("test");
        layer.store_put(RequestAttempts::new(1));
        layer.store_put(RetryConfig::standard().with_max_attempts(5));
        layer.store_put(custom_bucket);
        let cfg = ConfigBag::of_layers(vec![layer]);
        let token_bucket = cfg.load::<TokenBucket>().unwrap().clone();

        let strategy = StaticRetryStrategy::new();

        // First retry with TransientError should cost 25 tokens (timeout_retry_cost)
        let result = strategy.should_attempt_retry(&ctx, &rc, &cfg).unwrap();
        assert!(matches!(result, ShouldAttempt::YesAfterDelay(_)));
        assert_eq!(token_bucket.available_permits(), 75); // 100 - 25

        // Change to ServerError for next attempt
        let rc2 = RuntimeComponentsBuilder::for_tests()
            .with_retry_classifier(SharedRetryClassifier::new(AlwaysRetry(
                ErrorKind::ServerError,
            )))
            .with_time_source(Some(SystemTimeSource::new()))
            .build()
            .unwrap();

        let mut layer2 = Layer::new("test");
        layer2.store_put(RequestAttempts::new(2));
        let cfg2 = ConfigBag::of_layers(vec![layer, layer2]);

        // Second retry with ServerError should cost 10 tokens (retry_cost)
        let result2 = strategy.should_attempt_retry(&ctx, &rc2, &cfg2).unwrap();
        assert!(matches!(result2, ShouldAttempt::YesAfterDelay(_)));
        assert_eq!(token_bucket.available_permits(), 65); // 75 - 10
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn custom_token_costs_exhaustion() {
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::other("test error")));

        let rc = RuntimeComponentsBuilder::for_tests()
            .with_retry_classifier(SharedRetryClassifier::new(AlwaysRetry(
                ErrorKind::ServerError,
            )))
            .with_time_source(Some(SystemTimeSource::new()))
            .build()
            .unwrap();

        // Custom bucket with very high costs to test exhaustion behavior
        let custom_bucket = TokenBucket::builder()
            .capacity(20)
            .retry_cost(15) // High cost
            .timeout_retry_cost(30) // Very high cost
            .build();

        let mut layer = Layer::new("test");
        layer.store_put(RequestAttempts::new(1));
        layer.store_put(RetryConfig::standard().with_max_attempts(5));
        layer.store_put(custom_bucket);
        let cfg = ConfigBag::of_layers(vec![layer]);
        let token_bucket = cfg.load::<TokenBucket>().unwrap().clone();

        let strategy = StaticRetryStrategy::new();

        // First retry should succeed (20 >= 15)
        let result = strategy.should_attempt_retry(&ctx, &rc, &cfg).unwrap();
        assert!(matches!(result, ShouldAttempt::YesAfterDelay(_)));
        assert_eq!(token_bucket.available_permits(), 5); // 20 - 15

        // Second retry should fail (5 < 15)
        let mut layer2 = Layer::new("test");
        layer2.store_put(RequestAttempts::new(2));
        let cfg2 = ConfigBag::of_layers(vec![layer, layer2]);

        let result2 = strategy.should_attempt_retry(&ctx, &rc, &cfg2).unwrap();
        assert_eq!(result2, ShouldAttempt::No); // Not enough tokens
        assert_eq!(token_bucket.available_permits(), 5); // No tokens consumed on failure
    }
}
