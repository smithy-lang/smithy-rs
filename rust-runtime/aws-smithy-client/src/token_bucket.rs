/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_types::retry::ErrorKind;
use std::fmt;

pub mod standard;

pub trait Token {
    /// Release this token back to the bucket.
    fn release(self);

    /// Forget this token, forever banishing it to the shadow realm, from whence no tokens return.
    fn forget(self);
}

pub trait TokenBucket {
    type Token;

    /// Attempt to acquire a token from the bucket. This will fail if the bucket has no more tokens.
    fn try_acquire(
        &self,
        previous_request_failed_because: Option<ErrorKind>,
    ) -> Result<Self::Token, TokenBucketError>;

    // TODO should this be exposed for non-`test` usage?
    /// Get the number of available tokens in the bucket.
    fn available(&self) -> usize;

    /// Refill the bucket with the given number of tokens.
    fn refill(&self, tokens: usize);
}

#[derive(Debug)]
pub enum TokenBucketError {
    NoTokens,
    Bug(String),
}

impl fmt::Display for TokenBucketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTokens => write!(f, "No more tokens are left in the bucket."),
            Self::Bug(msg) => write!(f, "you've encountered a bug that needs reporting: {}", msg),
        }
    }
}

impl std::error::Error for TokenBucketError {}
