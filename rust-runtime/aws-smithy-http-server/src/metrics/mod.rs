/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Collection of standard server metrics.

use std::time::{Instant, SystemTime};

pub use self::{
    request_ext::RequestExt,
    service::{MetricsService, MetricsServiceFuture},
};

mod body;
mod request_ext;
mod service;
mod state;

/// Standard server metrics collected for a single request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RequestMetrics {
    start_time: Instant,
    end_time: Instant,
    start_system_time: SystemTime,
}

impl RequestMetrics {
    /// When the service began handling the request, as both a monotonic and a wall-clock time.
    ///
    /// Other timestamps are only monotonic. To convert one to wall-clock time, add its offset
    /// from the monotonic start time to the wall-clock start time:
    ///
    /// ```
    /// # use aws_smithy_http_server::metrics::RequestMetrics;
    /// # use std::time::SystemTime;
    /// fn end_system_time(metrics: &RequestMetrics) -> SystemTime {
    ///     let (start, start_system_time) = metrics.start_time();
    ///     start_system_time + (metrics.end_time() - start)
    /// }
    /// ```
    pub fn start_time(&self) -> (Instant, SystemTime) {
        (self.start_time, self.start_system_time)
    }

    /// When the request finished: its response was fully sent, sending the response failed, or
    /// handling the request was abandoned.
    pub fn end_time(&self) -> Instant {
        self.end_time
    }
}
