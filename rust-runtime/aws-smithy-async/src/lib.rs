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

use crate::rt::sleep::AsyncSleep;
use aws_smithy_runtime_api::config_bag::{Accessor, Setter, Storable, StoreReplace};
use std::sync::Arc;

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

#[derive(Debug)]
struct StorableSleep(Arc<dyn AsyncSleep>);

impl Storable for StorableSleep {
    type Storer = StoreReplace<StorableSleep>;
}

/// Async configuration accessor trait
pub trait AsyncConfig: Accessor {
    /// Configured sleep implementation
    fn sleep_impl(&self) -> Option<Arc<dyn AsyncSleep>> {
        self.config().load::<StorableSleep>().map(|s| s.0.clone())
    }
}

pub trait AsyncConfigBuilder: Setter {
    fn sleep_impl(mut self, sleep_impl: Arc<dyn AsyncSleep>) -> Self
    where
        Self: Sized,
    {
        self.set_sleep_impl(Some(sleep_impl));
        self
    }

    fn set_sleep_impl(&mut self, sleep_impl: Option<Arc<dyn AsyncSleep>>) -> &mut Self {
        self.config().store_put(sleep_impl.map(StorableSleep));
        self
    }
}
