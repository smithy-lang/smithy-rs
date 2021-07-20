/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Provides an [`AsyncSleep`] trait that returns a future that sleeps for a given duration,
//! and implementations of `AsyncSleep` for different async runtimes.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// Async trait with a `sleep` function.
pub trait AsyncSleep: Send + Sync {
    /// Returns a future that sleeps for the given `duration` of time.
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// Implementation of [`AsyncSleep`] for Tokio.
#[cfg(feature = "sleep-tokio")]
#[derive(Default)]
pub struct TokioSleep;

#[cfg(feature = "sleep-tokio")]
impl TokioSleep {
    pub fn new() -> TokioSleep {
        Default::default()
    }
}

#[cfg(feature = "sleep-tokio")]
impl AsyncSleep for TokioSleep {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(duration))
    }
}
