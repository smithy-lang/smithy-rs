/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::time::Duration;

pub enum RetryType {
    /// This is a connection level error such as a socket timeout, socket connect error,
    /// tls negotiation timeout etc...
    ///
    /// Typically these should never be applied for non-idempotent request types
    /// since in this scenario, it's impossible to know whether the operation had
    /// a side effect on the server.
    TransientError,

    /// An error where the server explicitly told the client to back off, such as a 429 or 503 HTTP error.
    ThrottlingError,

    /// Server error that isn't explicitly throttling but is considered by the client
    /// to be something that should be retried.
    ServerError,

    /// Doesn't count against any budgets. This could be something like a 401 challenge in Http.
    ClientError,

    /// An explicit retry in a set duration. This allows waiters
    /// to be a special case of retries
    Explicit(Duration)
}

pub trait RetryPolicy<T> {
    fn should_retry(&self, input: &T) -> Option<RetryType>;
}

/// () is the default policy: never retry
impl<T> RetryPolicy<T> for () {
    fn should_retry(&self, _: &T) -> Option<RetryType> {
        None
    }
}
