/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A body-wrapping type that ensures data is being streamed faster than some lower limit.
//!
//! If data is being streamed too slowly, this body type will emit an error next time it's polled.

mod throughput;

use aws_smithy_async::time::{SharedTimeSource, TimeSource};
use aws_smithy_runtime_api::shared::IntoShared;
use bytes::Buf;
use http::HeaderMap;
use std::collections::VecDeque;
use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};
use throughput::Throughput;

// Chosen arbitrarily.
const LOG_WINDOW_SIZE: usize = 16;

pin_project_lite::pin_project! {
    /// A body-wrapper that will ensure that the wrapped body is emitting bytes faster than some
    /// `minimum_throughput`.
    pub struct MinimumThroughputBody<InnerBody> {
        #[pin]
        inner: InnerBody,
        // A record of when and how much data was read
        throughput_logs: VecDeque<(SystemTime, u64)>,
        // The minimum acceptable throughput. If the amount of data per unit of time returned is
        // less that this, an error will be returned instead.
        minimum_throughput: Throughput,
        time_source: SharedTimeSource,
    }
}

impl<T> MinimumThroughputBody<T> {
    // If this function returns:
    // - `None`: We couldn't calculate a throughput because we don't have good data yet.
    // - `Some(throughput)`: We have good data, and we can calculate a throughput.
    // -- `Err(_)`: A bug occurred.
    fn calculate_throughput(&self) -> Result<Option<Throughput>, Error> {
        if let Some((earliest_time, _)) = self.throughput_logs.front() {
            let now = self.time_source.now();
            let time_elapsed_since_earliest_poll = now
                .duration_since(*earliest_time)
                .map_err(|err| Error::TimeTravel(err.into()))?;

            // This check ensures we that the data we're looking at covers a good range of time.
            // If not, then we don't calculate a throughput.
            if time_elapsed_since_earliest_poll < self.minimum_throughput.per_time_elapsed {
                return Ok(None);
            }

            let total_bytes_logged =
                self.throughput_logs
                    .iter()
                    .fold(0, |acc, (_, bytes_read)| acc + bytes_read) as f64;

            Ok(Some(Throughput {
                bytes_read: total_bytes_logged,
                per_time_elapsed: time_elapsed_since_earliest_poll,
            }))
        } else {
            Ok(None)
        }
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
            throughput_logs: VecDeque::with_capacity(LOG_WINDOW_SIZE),
            minimum_throughput: minimum_throughput.into(),
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
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.as_mut().project();
        let poll_res = this.inner.poll_data(cx);

        if let Poll::Ready(Some(Ok(ref data))) = poll_res {
            let now = this.time_source.now();
            this.throughput_logs
                .push_back((now, data.remaining() as u64));
            // When the number of logs exceeds the window size, toss the oldest log.
            if this.throughput_logs.len() > LOG_WINDOW_SIZE {
                this.throughput_logs.pop_front();
            }
        };

        if let Some(actual_throughput) = self.calculate_throughput()? {
            // oh no, too slow!
            if actual_throughput < self.minimum_throughput {
                return Poll::Ready(Some(Err(Box::new(Error::ThroughputBelowMinimum {
                    expected: self.minimum_throughput,
                    actual: actual_throughput,
                }))));
            }
        }

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
        expected: Throughput,
        actual: Throughput,
    },
    TimeTravel(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ThroughputBelowMinimum { expected, actual } => {
                write!(
                    f,
                    "minimum throughput was specified at {expected}, but throughput of {actual} was observed",
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
