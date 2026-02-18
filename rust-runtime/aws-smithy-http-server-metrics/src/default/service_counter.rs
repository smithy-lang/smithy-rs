/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// A service-level counter for metrics that need state beyond a single request
#[derive(Default, Debug, Clone)]
pub(crate) struct ServiceCounter {
    inner: Arc<ServiceCounterInner>,
}
impl ServiceCounter {
    /// Increments the global count by 1, returning a guard that
    /// decrements the count on drop, and the new value
    pub(crate) fn increment(&self) -> (ServiceCounterGuard, usize) {
        let count = self.inner.increment();
        (ServiceCounterGuard(Arc::clone(&self.inner)), count)
    }
}

#[derive(Default, Debug, Clone)]
pub(crate) struct ServiceCounterInner {
    count: Arc<AtomicUsize>,
}
impl ServiceCounterInner {
    fn increment(&self) -> usize {
        self.count.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// Guard for [`ServiceCounter`] that decrements its count on drop
pub(crate) struct ServiceCounterGuard(Arc<ServiceCounterInner>);

impl Drop for ServiceCounterGuard {
    fn drop(&mut self) {
        self.0.count.fetch_sub(1, Ordering::Relaxed);
    }
}
