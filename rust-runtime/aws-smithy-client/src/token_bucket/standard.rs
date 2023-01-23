/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A token bucket intended for use with the standard smithy client retry policy.

use super::Token as TokenTrait;
use super::TokenBucket as TokenBucketTrait;
use super::TokenBucketError;
use aws_smithy_types::retry::{ErrorKind, RetryKind};
use std::sync::Arc;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::TryAcquireError;

/// The default number of tokens to start with
const DEFAULT_INITIAL_RETRY_TOKENS: usize = 500;
/// The amount of tokens to remove from the bucket when a timeout error occurs
const DEFAULT_TIMEOUT_ERROR_RETRY_COST: u32 = 10;
/// The amount of tokens to remove from the bucket when a throttling error occurs
const DEFAULT_RETRYABLE_ERROR_RETRY_COST: u32 = 5;
/// The amount of tokens to add to the bucket when a request succeeds on the first try
const SUCCESS_ON_FIRST_TRY_REFILL_AMOUNT: usize = 1;
/// Status codes representing retryable failures
const RETRYABLE_ERROR_STATUS_CODES: &[u16] = &[500, 502, 503, 504];

/// The token type of [`TokenBucket`].
#[derive(Debug)]
pub struct Token {
    permit: Option<OwnedSemaphorePermit>,
}

impl Token {
    fn new(permit: OwnedSemaphorePermit) -> Self {
        Self {
            permit: Some(permit),
        }
    }

    // Return an "empty" token for times when you need to return a token but there's no "cost"
    // associated with an action.
    fn empty() -> Self {
        Self { permit: None }
    }
}

impl TokenTrait for Token {
    fn release(self) {
        drop(self.permit)
    }

    fn forget(self) {
        match self.permit {
            Some(permit) => permit.forget(),
            None => (),
        }
    }
}

/// A token bucket implementation that uses a `tokio::sync::Semaphore` to track the number of tokens.
///
/// - Whenever a request succeeds on the first try, `<success_on_first_try_refill_amount>` token(s)
///     are added back to the bucket.
/// - When a request fails with a timeout error, `<timeout_error_cost>` token(s)
///     are removed from the bucket.
/// - When a request fails with a retryable error, `<retryable_error_cost>` token(s)
///     are removed from the bucket.
///
/// The number of tokens in the bucket will always be >= `0` and <= `<max_tokens>`.
#[derive(Clone, Debug)]
pub struct TokenBucket {
    inner: Arc<Semaphore>,
    max_tokens: usize,
    success_on_first_try_refill_amount: usize,
    timeout_error_cost: u32,
    retryable_error_cost: u32,
}

impl TokenBucket {
    /// Create a new `TokenBucket` using builder methods.
    pub fn builder() -> Builder {
        Builder::default()
    }
}

/// A builder for `TokenBucket`s.
#[derive(Default, Debug)]
pub struct Builder {
    starting_tokens: Option<usize>,
    max_tokens: Option<usize>,
    success_on_first_try_refill_amount: Option<usize>,
    timeout_error_cost: Option<u32>,
    retryable_error_cost: Option<u32>,
}

impl Builder {
    /// The number of tokens the bucket will start with. Defaults to 500.
    pub fn starting_tokens(mut self, starting_tokens: usize) -> Self {
        self.starting_tokens = Some(starting_tokens);
        self
    }

    /// The maximum number of tokens that the bucket can hold.
    /// Defaults to the value of `starting_tokens`.
    pub fn max_tokens(mut self, max_tokens: usize) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Whenever a request succeeds on the first attempt, the bucket will be refilled by this amount
    /// of tokens. Defaults to 1.
    pub fn success_on_first_try_refill_amount(
        mut self,
        success_on_first_try_refill_amount: usize,
    ) -> Self {
        self.success_on_first_try_refill_amount = Some(success_on_first_try_refill_amount);
        self
    }

    /// How many tokens to remove from the bucket when a request fails due to a timeout error.
    /// Defaults to 10.
    pub fn timeout_error_cost(mut self, timeout_error_cost: u32) -> Self {
        self.timeout_error_cost = Some(timeout_error_cost);
        self
    }

    /// How many tokens to remove from the bucket when a request fails due to a retryable error that
    /// isn't timeout-related. Defaults to 5.
    pub fn retryable_error_cost(mut self, retryable_error_cost: u32) -> Self {
        self.retryable_error_cost = Some(retryable_error_cost);
        self
    }

    /// Build this builder. Unset fields will be set to their default values.
    pub fn build(self) -> TokenBucket {
        let starting_tokens = self.starting_tokens.unwrap_or(DEFAULT_INITIAL_RETRY_TOKENS);
        let max_tokens = self.max_tokens.unwrap_or(starting_tokens);
        let success_on_first_try_refill_amount = self
            .success_on_first_try_refill_amount
            .unwrap_or(SUCCESS_ON_FIRST_TRY_REFILL_AMOUNT);
        let timeout_error_cost = self
            .timeout_error_cost
            .unwrap_or(DEFAULT_TIMEOUT_ERROR_RETRY_COST);
        let retryable_error_cost = self
            .retryable_error_cost
            .unwrap_or(DEFAULT_RETRYABLE_ERROR_RETRY_COST);

        TokenBucket {
            inner: Arc::new(Semaphore::new(starting_tokens)),
            max_tokens,
            success_on_first_try_refill_amount,
            timeout_error_cost,
            retryable_error_cost,
        }
    }
}

impl TokenBucketTrait for TokenBucket {
    type Token = Token;

    fn try_acquire(
        &self,
        previous_response_kind: Option<RetryKind>,
    ) -> Result<Self::Token, TokenBucketError> {
        let number_of_tokens_to_acquire = match previous_response_kind {
            None => {
                // Return an empty token because the quota layer lifecycle expects a for each
                // request even though the standard token bucket only requires tokens for retry
                // attempts.
                return Ok(Token::empty());
            }

            Some(retry_kind) => match retry_kind {
                RetryKind::Unnecessary => {
                    unreachable!("BUG: asked for a token to retry a successful request")
                }
                RetryKind::UnretryableFailure => {
                    unreachable!("BUG: asked for a token to retry an un-retryable request")
                }
                RetryKind::Explicit(_) => self.retryable_error_cost,
                RetryKind::Error(error_kind) => match error_kind {
                    ErrorKind::ThrottlingError => self.timeout_error_cost,
                    ErrorKind::ServerError | ErrorKind::TransientError => self.retryable_error_cost,
                    ErrorKind::ClientError => unreachable!(
                        "BUG: asked for a token to retry a request that failed due to user error"
                    ),
                    _ => unreachable!(
                        "A new variant '{:?}' was added to ErrorKind, please handle it",
                        error_kind
                    ),
                },
                _ => unreachable!(
                    "A new variant '{:?}' was added to RetryKind, please handle it",
                    retry_kind
                ),
            },
        };

        match self
            .inner
            .clone()
            .try_acquire_many_owned(number_of_tokens_to_acquire)
        {
            Ok(permit) => Ok(Token::new(permit)),
            Err(TryAcquireError::NoPermits) => Err(TokenBucketError::NoTokens),
            Err(other) => Err(TokenBucketError::Bug(other.to_string())),
        }
    }

    fn available(&self) -> usize {
        self.inner.available_permits()
    }

    fn refill(&self, tokens: usize) {
        // Ensure the bucket doesn't overflow by limiting the amount of tokens to add, if necessary.
        let amount_to_add = (self.available() + tokens).min(self.max_tokens) - self.available();
        if amount_to_add > 0 {
            self.inner.add_permits(amount_to_add)
        }
    }
}

#[cfg(test)]
mod test {
    use super::{
        TokenBucket, DEFAULT_INITIAL_RETRY_TOKENS, DEFAULT_RETRYABLE_ERROR_RETRY_COST,
        DEFAULT_TIMEOUT_ERROR_RETRY_COST,
    };
    use crate::token_bucket::{Token, TokenBucket as TokenBucketTrait};

    #[test]
    fn bucket_works() {
        let bucket = TokenBucket::builder().build();
        assert_eq!(bucket.available(), DEFAULT_INITIAL_RETRY_TOKENS);

        let token = bucket
            .try_acquire(Some(ResponseKind::RetryableError))
            .unwrap();
        assert_eq!(
            bucket.available(),
            DEFAULT_INITIAL_RETRY_TOKENS - DEFAULT_RETRYABLE_ERROR_RETRY_COST as usize
        );
        token.release();

        let token = bucket
            .try_acquire(Some(ResponseKind::TimeoutError))
            .unwrap();
        assert_eq!(
            bucket.available(),
            DEFAULT_INITIAL_RETRY_TOKENS - DEFAULT_TIMEOUT_ERROR_RETRY_COST as usize
        );
        token.forget();
        assert_eq!(
            bucket.available(),
            DEFAULT_INITIAL_RETRY_TOKENS - DEFAULT_TIMEOUT_ERROR_RETRY_COST as usize
        );

        bucket.refill(DEFAULT_TIMEOUT_ERROR_RETRY_COST as usize);
        assert_eq!(bucket.available(), DEFAULT_INITIAL_RETRY_TOKENS);
    }
}
