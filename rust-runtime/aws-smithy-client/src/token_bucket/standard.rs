/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::Token as TokenTrait;
use super::TokenBucket as TokenBucketTrait;
use super::TokenBucketError;
use aws_smithy_types::retry::ErrorKind;
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
const SUCCESS_ON_FIRST_TRY_REFILL_AMOUNT: u32 = 1;

pub struct Token {
    permit: OwnedSemaphorePermit,
}

impl TokenTrait for Token {
    fn release(self) {
        drop(self.permit)
    }

    fn forget(self) {
        self.permit.forget()
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
#[derive(Clone)]
pub struct TokenBucket {
    inner: Arc<Semaphore>,
    max_tokens: usize,
    success_on_first_try_refill_amount: u32,
    timeout_error_cost: u32,
    retryable_error_cost: u32,
}

impl TokenBucket {
    pub fn builder() -> Builder {
        Builder::default()
    }
}

#[derive(Default)]
pub struct Builder {
    starting_tokens: Option<usize>,
    max_tokens: Option<usize>,
    success_on_first_try_refill_amount: Option<u32>,
    timeout_error_cost: Option<u32>,
    retryable_error_cost: Option<u32>,
}

impl Builder {
    pub fn starting_tokens(mut self, starting_tokens: usize) -> Self {
        self.starting_tokens = Some(starting_tokens);
        self
    }

    pub fn max_tokens(mut self, max_tokens: usize) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn success_on_first_try_refill_amount(
        mut self,
        success_on_first_try_refill_amount: u32,
    ) -> Self {
        self.success_on_first_try_refill_amount = Some(success_on_first_try_refill_amount);
        self
    }

    pub fn timeout_error_cost(mut self, timeout_error_cost: u32) -> Self {
        self.timeout_error_cost = Some(timeout_error_cost);
        self
    }

    pub fn retryable_error_cost(mut self, retryable_error_cost: u32) -> Self {
        self.retryable_error_cost = Some(retryable_error_cost);
        self
    }

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
        previous_request_failed_because: Option<ErrorKind>,
    ) -> Result<Self::Token, TokenBucketError> {
        match self.inner.clone().try_acquire_owned() {
            Ok(permit) => Ok(Token { permit }),
            Err(TryAcquireError::NoPermits) => Err(TokenBucketError::NoTokens),
            Err(other) => Err(TokenBucketError::Bug(other.to_string())),
        }
    }

    fn refill(&self, tokens: usize) {
        self.inner.add_permits(tokens)
    }

    fn available(&self) -> usize {
        self.inner.available_permits()
    }
}

#[cfg(test)]
mod test {
    use aws_smithy_types::retry::ErrorKind;

    use super::{
        TokenBucket, DEFAULT_INITIAL_RETRY_TOKENS, DEFAULT_RETRYABLE_ERROR_RETRY_COST,
        DEFAULT_TIMEOUT_ERROR_RETRY_COST,
    };
    use crate::token_bucket::{Token, TokenBucket as TokenBucketTrait};

    #[test]
    fn bucket_works() {
        let bucket = TokenBucket::builder().build();
        assert_eq!(bucket.available(), DEFAULT_INITIAL_RETRY_TOKENS);

        let token = bucket.try_acquire(Some(ErrorKind::ServerError)).unwrap();
        assert_eq!(
            bucket.available(),
            DEFAULT_INITIAL_RETRY_TOKENS - DEFAULT_RETRYABLE_ERROR_RETRY_COST as usize
        );
        token.release();

        let token = bucket.try_acquire(Some(ErrorKind::TransientError)).unwrap();
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
