/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Types and traits related to token buckets. Token buckets are used to limit the amount of
//! requests a client sends in order to avoid getting throttled. Token buckets can also act as a
//! form of concurrency control if a token is required to send a new request (as opposed to retry
//! requests only).

use aws_smithy_types::retry::RetryKind;
use std::fmt;

pub mod standard;

/// A trait implemented by types that represent a token dispensed from a [`TokenBucket`].
pub trait Token {
    /// Release this token back to the bucket. This should be called if the related request succeeds.
    fn release(self);

    /// Forget this token, forever banishing it to the shadow realm, from whence no tokens return.
    /// This should be called if the related request fails.
    fn forget(self);
}

/// This trait is implemented by types that act as token buckets. Token buckets are used to regulate
/// the amount of requests sent by clients. Different token buckets may apply different strategies
/// to manage the number of tokens in a bucket.
///
/// related: [`Token`], [`Error`]
pub trait TokenBucket {
    /// The type of tokens this bucket dispenses.
    type Token: Token;

    /// Attempt to acquire a token from the bucket. This will fail if the bucket has no more tokens.
    fn try_acquire(&self, previous_response_kind: Option<RetryKind>) -> Result<Self::Token, Error>;

    /// Get the number of available tokens in the bucket.
    fn available(&self) -> usize;

    /// Refill the bucket with the given number of tokens.
    fn refill(&self, tokens: usize);
}

/// Errors related to a token bucket.
#[derive(Debug)]
pub enum Error {
    /// A token was requested but there were no tokens left in the bucket.
    NoTokens,
    /// This error should never occur and is a bug. Please report it.
    Bug(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTokens => write!(f, "No more tokens are left in the bucket."),
            Self::Bug(msg) => write!(f, "you've encountered a bug that needs reporting: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::TokenBucket;
    use crate::standard;

    #[test]
    fn token_bucket_trait_is_dyn_safe() {
        let tb: Box<dyn TokenBucket<Token = standard::Token>> =
            Box::new(standard::TokenBucket::builder().build());
    }
}
