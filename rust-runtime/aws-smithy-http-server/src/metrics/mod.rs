/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Collection of standard server metrics.

use std::time::{Instant, SystemTime};

use crate::shape_id::ShapeId;

#[doc(hidden)]
pub use self::outcome::ModeledOutcome;
pub(crate) use self::state::{collecting, record_operation};
pub use self::{
    outcome::{ErrorOutcome, Fault, Outcome, OutcomeSource, SuccessOutcome},
    request_ext::RequestExt,
    service::{MetricsService, MetricsServiceFuture},
};

mod body;
mod outcome;
mod request_ext;
mod service;
mod state;
#[cfg(test)]
mod test_util;

/// Standard server metrics collected for a single request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RequestMetrics {
    start_time: Instant,
    end_time: Instant,
    start_system_time: SystemTime,
    service: ShapeId,
    service_version: Option<&'static str>,
    outcome: Option<Outcome>,
    operation: Option<ShapeId>,
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

    /// The [`ShapeId`] of the service that handled the request.
    pub fn service(&self) -> &ShapeId {
        &self.service
    }

    /// The version of the service that handled the request, if its model specifies one.
    pub fn service_version(&self) -> Option<&str> {
        self.service_version
    }

    /// The [`ShapeId`] of the operation the request was routed to, or `None` if it was not routed to
    /// an operation.
    pub fn operation(&self) -> Option<&ShapeId> {
        self.operation.as_ref()
    }

    /// How the request turned out.
    ///
    /// See the documentation for [Outcome] for details on how outcomes are classified.
    ///
    /// This method may return `None` in some rare circumstances:
    ///
    /// - The client disconnects before a response was produced.
    /// - The server is [gracefully shut down] and the [graceful shutdown timeout] expires while a
    ///   request is in flight.
    /// - An uncaught panic unwinds through [MetricsServiceFuture].
    /// - Custom middleware surrounding the Smithy service drops [MetricsServiceFuture] before it
    ///   yields a response. (For example, a bespoke implementation of graceful shutdown.)
    /// - [MetricsService] has been manually wrapped around a custom non-Smithy [Tower service] that
    ///   returns `Err`. (Smithy services cannot return `Err` because their error type is
    ///   [Infallible].)
    ///
    /// [gracefully shut down]: crate::serve::Serve::with_graceful_shutdown
    /// [graceful shutdown timeout]: crate::serve::ServeWithGracefulShutdown::with_shutdown_timeout
    /// [Tower service]: tower::Service
    /// [Infallible]: std::convert::Infallible
    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }
}
