/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The producer handle that the metrics layer places in each request's
//! extensions.

use std::ops::Deref;

use metrique_util::MetricsPoolHandle;

/// Producer handle for the current request's metrics pool, stored in the
/// request extensions by [`MetricsLayer`](crate::MetricsLayer).
///
/// Independently-owned middleware and libraries that cannot name the service's
/// metrics entry type pull this out of the request extensions and append
/// prefixed child metrics, which flatten into the single per-request entry when
/// the request finishes:
///
/// ```rust,ignore
/// if let Some(pool) = req.extensions().get::<SmithyMetricsPool>() {
///     pool.with_prefix(["middleware"]).append(MiddlewareMetrics { .. });
/// }
/// ```
///
/// This is a newtype over the underlying pool handle so that it occupies its own
/// slot in the extensions and cannot be confused with another middleware's own
/// handle type.
#[derive(Clone, Debug)]
pub struct SmithyMetricsPool(MetricsPoolHandle);

impl SmithyMetricsPool {
    pub(crate) fn new(handle: MetricsPoolHandle) -> Self {
        Self(handle)
    }
}

impl Deref for SmithyMetricsPool {
    type Target = MetricsPoolHandle;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
