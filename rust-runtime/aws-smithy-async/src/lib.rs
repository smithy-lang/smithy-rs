/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(clippy::derive_partial_eq_without_eq)]
#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    unreachable_pub,
    rust_2018_idioms
)]

//! Future utilities and runtime-agnostic abstractions for smithy-rs.
//!
//! Async runtime specific code is abstracted behind async traits, and implementations are
//! provided via feature flag. For now, only Tokio runtime implementations are provided.

pub mod future;
pub mod rt;

/// Given an `Instant` and a `Duration`, assert time elapsed since `Instant` is equal to `Duration`.
/// This macro allows for a 5ms margin of error.
///
/// # Example
///
/// ```rust,ignore
/// let now = std::time::Instant::now();
/// let _ = some_function_that_always_takes_five_seconds_to_run().await;
/// assert_elapsed!(now, std::time::Duration::from_secs(5));
/// ```
#[macro_export]
macro_rules! assert_elapsed {
    ($start:expr, $dur:expr) => {
        assert_elapsed!($start, $dur, std::time::Duration::from_millis(5));
    };
    ($start:expr, $dur:expr, $margin_of_error:expr) => {{
        let elapsed = $start.elapsed();
        // type ascription improves compiler error when wrong type is passed
        let lower: std::time::Duration = $dur;
        let margin_of_error: std::time::Duration = $margin_of_error;

        // Handles ms rounding
        assert!(
            elapsed >= lower && elapsed <= lower + margin_of_error,
            "actual = {:?}, expected = {:?}",
            elapsed,
            lower
        );
    }};
}

use crate::rt::sleep::AsyncSleep;
use aws_smithy_runtime_api::config_bag::{ConfigBag, FrozenConfigBag};
use aws_smithy_runtime_api::storable;
use std::sync::Arc;

#[derive(Debug)]
struct AsyncSleepStorable(Arc<dyn AsyncSleep>);
storable!(AsyncSleepStorable, mode: replace);

/// Async Runtime configuration
pub struct AsyncConfiguration {
    inner: FrozenConfigBag,
}

pub struct AsyncConfigurationBuilder<'a> {
    bag: &'a mut ConfigBag,
}

impl<'a> AsyncConfigurationBuilder<'a> {
    pub fn from_bag(bag: &'a mut ConfigBag) -> Self {
        Self { bag }
    }

    pub fn set_sleep_impl(&mut self, sleep: Option<Arc<dyn AsyncSleep>>) {
        self.bag.store_or_unset(sleep.map(AsyncSleepStorable));
    }
}

impl AsyncConfiguration {
    pub fn from_bag(bag: &FrozenConfigBag) -> Self {
        Self { inner: bag.clone() }
    }

    /// Retrieve an Async Sleep from the bag
    pub fn async_sleep(&self) -> Option<Arc<dyn AsyncSleep>> {
        self.inner.load::<AsyncSleepStorable>().map(|s| s.0.clone())
    }
}
