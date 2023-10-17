/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A body-wrapping type that ensures data is being streamed faster than some lower limit.
//!
//! If data is being streamed too slowly, this body type will emit an error next time it's polled.

mod throughput;

use aws_smithy_async::rt::sleep::{AsyncSleep, SharedAsyncSleep};
use aws_smithy_async::time::{SharedTimeSource, TimeSource};
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::shared::IntoShared;
use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};
use throughput::Throughput;
use tokio::select;

// Chosen arbitrarily.
const LOG_WINDOW_SIZE: usize = 16;

pin_project_lite::pin_project! {
    struct MinimumThroughputBody<D, T> {
        #[pin]
        data_fut: D,
        #[pin]
        trailers_fut: T,
    }
}

#[derive(Clone)]
struct ThroughputLogs {
    window_size: usize,
    inner: Arc<Mutex<VecDeque<(SystemTime, u64)>>>,
}

impl ThroughputLogs {
    fn new(window_size: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(window_size))),
            window_size,
        }
    }

    fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().is_empty()
    }

    fn push(&self, throughput: (SystemTime, u64)) {
        let mut inner = self.inner.lock().unwrap();
        inner.push_back(throughput);

        // When the number of logs exceeds the window size, toss the oldest log.
        if inner.len() > self.window_size {
            inner.pop_front();
        }
    }

    // If this function returns:
    // - `None`: We couldn't calculate a throughput because we don't have good data yet.
    // - `Some(throughput)`: We have good data, and we can calculate a throughput.
    // -- `Err(_)`: A bug occurred.
    fn calculate_throughput(
        &self,
        now: SystemTime,
        min_duration_elapsed: Duration,
    ) -> Result<Option<Throughput>, Error> {
        let mut inner = self.inner.lock().unwrap();

        if let Some((earliest_time, _)) = inner.front() {
            let time_elapsed_since_earliest_poll = now
                .duration_since(*earliest_time)
                .map_err(|err| Error::TimeTravel(err.into()))?;

            // This check ensures we that the data we're looking at covers a good range of time.
            // If not, then we don't calculate a throughput.
            if time_elapsed_since_earliest_poll < min_duration_elapsed {
                return Ok(None);
            }

            let total_bytes_logged = inner
                .iter()
                .fold(0, |acc, (_, bytes_read)| acc + bytes_read)
                as f64;

            Ok(Some(Throughput {
                bytes_read: total_bytes_logged,
                per_time_elapsed: time_elapsed_since_earliest_poll,
            }))
        } else {
            Ok(None)
        }
    }
}

impl<D, T> MinimumThroughputBody<D, T> {
    pub fn new<B>(
        time_source: impl TimeSource + 'static,
        async_sleep: impl AsyncSleep + 'static,
        mut body: B,
        minimum_throughput: (u64, Duration),
    ) -> Self
    where
        B: http_body::Body<Data = bytes::Bytes, Error = BoxError> + Unpin,
        D: Future<Output = Option<Result<B::Data, B::Error>>>,
        T: Future<Output = Result<Option<http::HeaderMap<http::HeaderValue>>, B::Error>>,
    {
        let throughput_logs = ThroughputLogs::new(LOG_WINDOW_SIZE);
        let minimum_throughput: Throughput = minimum_throughput.into();
        let shared_sleep: SharedAsyncSleep = async_sleep.into_shared();
        let shared_time: SharedTimeSource = time_source.into_shared();

        let data_fut = body.data();
        // Wrap the data future in our throughput-checking future.
        let data_fut = async {
            let async_sleep = shared_sleep.clone();
            let time_source = shared_time.clone();
            let throughput_logs = throughput_logs.clone();
            let tick_rate = minimum_throughput.per_time_elapsed;
            loop {
                let sleep_fut = async_sleep.sleep(tick_rate);
                select! {
                    sleep = sleep_fut => {
                        // We need at least one log to calc throughput, so ensure
                        // one is present. If we already have at least one log,
                        // then logging that we received no data is implicit and
                        // unnecessary.
                        if throughput_logs.is_empty() {
                            let now = time_source.now();
                            throughput_logs.push((now, 0));
                        }
                    },
                    data = &mut data_fut => {
                        break data;
                    }
                }

                let now = time_source.now();
                let actual_throughput = throughput_logs
                    .calculate_throughput(now, tick_rate)
                    .expect("time travel is impossible");
                match actual_throughput {
                    Some(actual_throughput) if actual_throughput < minimum_throughput => {
                        break Some(Err(Box::new(Error::ThroughputBelowMinimum {
                            expected: minimum_throughput,
                            actual: actual_throughput,
                        })));
                    }
                    _ => continue,
                }
            }
        };

        let trailers_fut = body.trailers();
        // Wrap the trailers future in our throughput-checking future.
        let trailers_fut = async {
            let async_sleep = shared_sleep.clone();
            let time_source = shared_time.clone();
            let throughput_logs = throughput_logs.clone();
            let tick_rate = minimum_throughput.per_time_elapsed;
            loop {
                let sleep_fut = async_sleep.sleep(tick_rate);
                select! {
                    sleep = sleep_fut => {
                        // We need at least one log to calc throughput, so ensure
                        // one is present. If we already have at least one log,
                        // then logging that we received no data is implicit and
                        // unnecessary.
                        if throughput_logs.is_empty() {
                            let now = time_source.now();
                            throughput_logs.push((now, 0));
                        }
                    },
                    trailers = &mut trailers_fut => {
                        break trailers;
                    }
                }

                let now = time_source.now();
                let actual_throughput = throughput_logs
                    .calculate_throughput(now, tick_rate)
                    .expect("time travel is impossible");
                match actual_throughput {
                    Some(actual_throughput) if actual_throughput < minimum_throughput => {
                        break Err(Box::new(Error::ThroughputBelowMinimum {
                            expected: minimum_throughput,
                            actual: actual_throughput,
                        }));
                    }
                    _ => continue,
                }
            }
        };

        Self {
            data_fut,
            trailers_fut,
        }
    }
}

impl<D, T> http_body::Body for MinimumThroughputBody<D, T>
where
    D: Future<Output = Option<Result<bytes::Bytes, BoxError>>>,
    T: Future<Output = Result<Option<http::HeaderMap<http::HeaderValue>>, BoxError>>,
{
    type Data = bytes::Bytes;
    type Error = BoxError;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        self.project().data_fut.poll(cx)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        self.project().trailers_fut.poll(cx)
    }
}

#[derive(Debug)]
enum Error {
    ThroughputBelowMinimum {
        expected: Throughput,
        actual: Throughput,
    },
    TimeTravel(BoxError),
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

#[cfg(test)]
mod tests {
    use super::{Error, MinimumThroughputBody};
    use aws_smithy_async::test_util::instant_time_and_sleep;
    use http::HeaderMap;
    use http_body::Body;
    use std::error::Error as StdError;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::{Duration, SystemTime};

    struct NeverBody;

    impl Body for NeverBody {
        type Data = bytes::Bytes;
        type Error = Box<(dyn StdError + Send + Sync + 'static)>;

        fn poll_data(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
            Poll::Pending
        }

        fn poll_trailers(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
            todo!()
        }
    }

    #[tokio::test()]
    async fn test_self_waking() {
        let (time_source, async_sleep) = instant_time_and_sleep(SystemTime::now());
        let mut body = MinimumThroughputBody::new(
            time_source.clone(),
            async_sleep.clone(),
            NeverBody,
            (1, Duration::from_secs(1)),
        );
        time_source.advance(Duration::from_secs(1));
        let actual_err = body.data().await.expect("next chunk exists").unwrap_err();
        let expected_err = Error::ThroughputBelowMinimum {
            expected: (1, Duration::from_secs(1)).into(),
            actual: (0, Duration::from_secs(1)).into(),
        };

        assert_eq!(expected_err.to_string(), actual_err.to_string());
    }
}
