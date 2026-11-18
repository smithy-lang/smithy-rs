/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Collection of standard server metrics.

use std::time::{Duration, Instant, SystemTime};

use crate::shape_id::ShapeId;

pub(crate) use self::state::{collecting, MetricsState, OperationGuard};
#[doc(hidden)]
pub use self::{operation::RecordOperation, outcome::ModeledOutcome};
pub use self::{
    outcome::{ErrorOutcome, Fault, Outcome, OutcomeSource, SuccessOutcome},
    request_ext::RequestExt,
    service::{MetricsService, MetricsServiceFuture},
};

mod body;
mod operation;
mod outcome;
mod request_ext;
mod service;
mod state;
#[cfg(test)]
mod test_util;

/// A point in time, as both a monotonic [`Instant`] and a wall-clock [`SystemTime`].
///
/// Use it to convert other [`Instant`]s to wall-clock time, by calling [`Self::system_time_of`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeAnchor {
    /// The point in time on the monotonic clock.
    pub instant: Instant,
    /// The same point in time on the wall clock.
    pub system_time: SystemTime,
}

impl TimeAnchor {
    /// Converts `instant` to wall-clock time, relative to this anchor. `instant` may be earlier or
    /// later than [`self.instant`](Self::instant).
    ///
    /// # Panics
    ///
    /// Panics if the result cannot be represented as a [`SystemTime`].
    pub fn system_time_of(&self, instant: Instant) -> SystemTime {
        if instant >= self.instant {
            self.system_time + (instant - self.instant)
        } else {
            self.system_time - (self.instant - instant)
        }
    }
}

/// Standard server metrics collected for a single request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RequestMetrics {
    start_time: TimeAnchor,
    end_time: Instant,
    service: ShapeId,
    service_version: Option<&'static str>,
    outcome: Option<Outcome>,
    operation: Option<ShapeId>,
    operation_duration: Duration,
    concurrent_requests_count: usize,
    request_id: Option<String>,
    status_code: Option<http::StatusCode>,
}

impl RequestMetrics {
    /// When the service began handling the request, as both a monotonic and a wall-clock time.
    ///
    /// Other timestamps are only monotonic. To convert one to wall-clock time, use
    /// [`TimeAnchor::system_time_of`]:
    ///
    /// ```
    /// # use aws_smithy_http_server::metrics::RequestMetrics;
    /// # use std::time::SystemTime;
    /// fn end_system_time(metrics: &RequestMetrics) -> SystemTime {
    ///     metrics.start_time().system_time_of(metrics.end_time())
    /// }
    /// ```
    pub fn start_time(&self) -> &TimeAnchor {
        &self.start_time
    }

    /// When the request finished: its response was fully sent, sending the response failed, or
    /// handling the request was abandoned.
    pub fn end_time(&self) -> Instant {
        self.end_time
    }

    /// The total wall-clock time spent executing the operation's [handler] and any [model plugins].
    ///
    /// If the operation was not executed at all (due to a request deserialization error, for
    /// example), the duration will be zero.
    ///
    /// The service may configure a [layer] or [HTTP plugin] that causes the operation to be
    /// executed multiple times. For example, a plugin might automatically retry failed operation
    /// invocations, or it might run multiple invocations in parallel (also known as hedging). In
    /// these cases, this method returns the total duration during which *at least* one invocation
    /// was running.
    ///
    /// [handler]: crate::operation::Handler
    /// [model plugins]: crate::plugin::ModelMarker
    /// [HTTP plugin]: crate::plugin::HttpMarker
    /// [layer]: tower::Layer
    pub fn operation_duration(&self) -> Duration {
        self.operation_duration
    }

    /// The number of requests to the Smithy service that had started but not finished, measured
    /// just before this request started. This request is *not* included in the count.
    pub fn concurrent_requests_count(&self) -> usize {
        // Excluding the current request is a departure from certain traditions, but is a deliberate
        // choice here. It has the nice property that under a Poisson arrival assumption, the
        // distribution of these values will approximate the distribution of a counter sampled at
        // random points in time.
        //
        // If a metrics publisher needs to include the current request, all they have to do is add
        // one to this count.
        self.concurrent_requests_count
    }

    /// The request's ID, if the service assigned one with [`ServerRequestIdProviderLayer`], which
    /// requires the `request-id` feature.
    ///
    /// The ID is collected when the request is routed to an operation, so a request that was not
    /// routed to an operation has none.
    ///
    #[cfg_attr(
        feature = "request-id",
        doc = "[`ServerRequestIdProviderLayer`]: crate::request::request_id::ServerRequestIdProviderLayer"
    )]
    #[cfg_attr(
        not(feature = "request-id"),
        doc = concat!(
            "[`ServerRequestIdProviderLayer`]: https://docs.rs/aws-smithy-http-server/",
            env!("CARGO_PKG_VERSION"),
            "/aws_smithy_http_server/request/request_id/struct.ServerRequestIdProviderLayer.html",
        ),
    )]
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// The HTTP status code of the response the service produced.
    ///
    /// This method may return `None` in rare circumstances where no response was produced. See the
    /// docs for [`Self::outcome`] for details about these circumstances.
    ///
    /// Metrics publishers should *not* use the status code to infer whether a request was a
    /// success, a client error, or a server error. Instead, use [`Self::outcome`] and match against
    /// the [`Outcome::result`] field.
    pub fn status_code(&self) -> Option<http::StatusCode> {
        self.status_code
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn time_anchor_converts_earlier_and_later_instants() {
        let anchor = TimeAnchor {
            instant: tokio::time::Instant::now().into_std(),
            system_time: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000),
        };
        let later = anchor.instant + Duration::from_secs(5);
        let earlier = anchor.instant - Duration::from_secs(5);
        assert_eq!(
            anchor.system_time_of(later),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_005)
        );
        assert_eq!(
            anchor.system_time_of(earlier),
            SystemTime::UNIX_EPOCH + Duration::from_secs(995)
        );
        assert_eq!(anchor.system_time_of(anchor.instant), anchor.system_time);
    }
}
