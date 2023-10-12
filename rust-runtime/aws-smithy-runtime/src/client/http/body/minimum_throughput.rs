/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A body-wrapping type that ensures data is being streamed faster than some lower limit.
//!
//! If data is being streamed too slowly, this body type will emit an error next time it's polled.

use aws_smithy_async::time::{SharedTimeSource, TimeSource};
use aws_smithy_runtime_api::shared::IntoShared;
use bytes::Buf;
use http::HeaderMap;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};

pin_project_lite::pin_project! {
    /// A body-wrapper that will ensure that the wrapped body is emitting bytes faster than some
    /// `minimum_throughput`.
    pub struct MinimumThroughputBody<InnerBody> {
        #[pin]
        inner: InnerBody,
        // A record of when and how much data was read
        throughput_logs: Vec<(SystemTime, u64)>,
        // The minimum acceptable throughput. If the amount of data per unit of time returned is
        // less that this, an error will be returned instead.
        minimum_throughput: (u64, Duration),
        time_source: SharedTimeSource,
    }
}

impl<T: http_body::Body> MinimumThroughputBody<T> {
    /// Given an HTTP body and a minimum throughput, create a new `MinimumThroughputBody`.
    pub fn new(
        time_source: impl TimeSource + 'static,
        body: T,
        minimum_throughput: (u64, Duration),
    ) -> Self {
        Self {
            inner: body,
            throughput_logs: Vec::new(),
            minimum_throughput,
            time_source: time_source.into_shared(),
        }
    }
}

impl<T> http_body::Body for MinimumThroughputBody<T>
where
    T: http_body::Body<Data = bytes::Bytes, Error = Box<dyn std::error::Error + Send + Sync>>,
{
    type Data = T::Data;
    type Error = T::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();

        let poll_res = this.inner.poll_data(cx);

        if let Poll::Ready(Some(Ok(ref data))) = poll_res {
            this.throughput_logs
                .push((this.time_source.now(), data.remaining() as u64));
        };

        let mut logs = this.throughput_logs.iter();
        if let Some((first_instant, first_bytes)) = logs.next() {
            let now = this.time_source.now();
            let time_elapsed_since_first_poll = now
                .duration_since(*first_instant)
                .map_err(|err| Box::new(Error::TimeTravel(err.into())))?;
            let mut total_bytes_read = *first_bytes;

            while let Some((_, bytes_read)) = logs.next() {
                total_bytes_read += bytes_read;
            }

            let minimum_bytes_per_second =
                this.minimum_throughput.0 as f64 / this.minimum_throughput.1.as_secs_f64();
            let actual_bytes_per_second =
                total_bytes_read as f64 / time_elapsed_since_first_poll.as_secs_f64();

            // oh no, too slow!
            if actual_bytes_per_second < minimum_bytes_per_second {
                return Poll::Ready(Some(Err(Box::new(Error::ThroughputBelowMinimum {
                    expected: this.minimum_throughput.clone(),
                    actual: (total_bytes_read, time_elapsed_since_first_poll),
                }))));
            }
        };

        poll_res
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        self.project().inner.poll_trailers(cx)
    }
}

#[derive(Debug)]
enum Error {
    ThroughputBelowMinimum {
        expected: (u64, Duration),
        actual: (u64, Duration),
    },
    TimeTravel(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ThroughputBelowMinimum { expected, actual } => {
                let expected = format_throughput(expected);
                let actual = format_throughput(actual);
                write!(
                    f,
                    "minimum throughput was specified at {}, but throughput of {} was observed",
                    expected, actual
                )
            }
            Self::TimeTravel(_) => write!(
                f,
                "negative time has elapsed while reading the inner body, this is a bug"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Format a given throughput as human-readable bytes per second
fn format_throughput(throughput: &(u64, Duration)) -> String {
    let b = throughput.0 as f64;
    let d = throughput.1.as_secs_f64();
    // The default float formatting behavior will ensure the a number like 2.000 is rendered as 2
    // while a number like 0.9982107441748642 will be rendered as 0.9982107441748642. This
    // multiplication and division will truncate a float to have a precision of no greater than 3.
    // For example, 0.9982107441748642 would become 0.999. This will fail for very large floats
    // but should suffice for the numbers we're dealing with.
    let bytes_per_second = ((b / d) * 1000.0).round() / 1000.0;

    format!("{bytes_per_second} B/s")
}
