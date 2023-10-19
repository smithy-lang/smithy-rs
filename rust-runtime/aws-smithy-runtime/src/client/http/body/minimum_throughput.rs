/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A body-wrapping type that ensures data is being streamed faster than some lower limit.
//!
//! If data is being streamed too slowly, this body type will emit an error next time it's polled.

mod throughput;

use aws_smithy_async::rt::sleep::Sleep;
use aws_smithy_async::rt::sleep::{AsyncSleep, SharedAsyncSleep};
use aws_smithy_async::time::{SharedTimeSource, TimeSource};
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::shared::IntoShared;
use std::fmt;
use std::future::Future;
use std::pin::{pin, Pin};
use std::task::{Context, Poll};
use std::time::Duration;
use throughput::{Throughput, ThroughputLogs};

pin_project_lite::pin_project! {
    /// A body-wrapper that ensures data is emitted from the body at a rate
    /// greater
    pub struct MinimumThroughputBody<B> {
        async_sleep: SharedAsyncSleep,
        time_source: SharedTimeSource,
        minimum_throughput: Throughput,
        throughput_logs: ThroughputLogs,
        #[pin]
        sleep_fut: Option<Sleep>,
        #[pin]
        inner: B,
    }
}

// Limit logs to ~1KB of memory overhead.
const SIZE_OF_ONE_LOG: usize = std::mem::size_of::<(std::time::SystemTime, u64)>();
const NUMBER_OF_LOGS_IN_ONE_KB: usize = 1024 / SIZE_OF_ONE_LOG;

impl<B> MinimumThroughputBody<B> {
    ///
    pub fn new(
        time_source: impl TimeSource + 'static,
        async_sleep: impl AsyncSleep + 'static,
        body: B,
        (bytes_read, per_time_elapsed): (u64, Duration),
    ) -> Self {
        let minimum_throughput = Throughput::new(bytes_read as f64, per_time_elapsed);

        Self {
            throughput_logs: ThroughputLogs::new(
                NUMBER_OF_LOGS_IN_ONE_KB,
                minimum_throughput.per_time_elapsed(),
            ),
            async_sleep: async_sleep.into_shared(),
            time_source: time_source.into_shared(),
            minimum_throughput: minimum_throughput.into(),
            inner: body,
            sleep_fut: None,
        }
    }
}

impl<B> http_body::Body for MinimumThroughputBody<B>
where
    B: http_body::Body<Data = bytes::Bytes, Error = BoxError>,
{
    type Data = bytes::Bytes;
    type Error = BoxError;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        // Attempt to read the data from the inner body, then update the
        // throughput logs.
        let mut this = self.as_mut().project();
        let (bytes_read, poll_res) = match this.inner.poll_data(cx) {
            Poll::Ready(Some(Ok(bytes))) => (bytes.len() as u64, Poll::Ready(Some(Ok(bytes)))),
            Poll::Pending => (0, Poll::Pending),
            // If we've read all the data or an error occurred, then return that result.
            res => return res,
        };
        this.throughput_logs
            .push((this.time_source.now(), bytes_read));

        // Check the sleep future to see if it needs refreshing.
        let mut sleep_fut = this.sleep_fut.take().unwrap_or_else(|| {
            this.async_sleep
                .sleep(this.minimum_throughput.per_time_elapsed())
        });
        match pin!(&mut sleep_fut).poll(cx) {
            Poll::Ready(()) => {
                // Whenever the sleep future expires, we replace it.
                sleep_fut = this
                    .async_sleep
                    .sleep(this.minimum_throughput.per_time_elapsed());
                // We also schedule a wake up for current task to ensure that
                // it gets polled at least one more time.
                cx.waker().wake_by_ref();
            }
            _ => (),
        };
        this.sleep_fut.replace(sleep_fut);

        // Calculate the current throughput and emit an error if it's too low.
        let actual_throughput = this.throughput_logs.calculate_throughput()?;
        let is_below_minimum_throughput = actual_throughput
            .map(|t| t < self.minimum_throughput)
            .unwrap_or_default();
        if is_below_minimum_throughput {
            Poll::Ready(Some(Err(Box::new(Error::ThroughputBelowMinimum {
                expected: self.minimum_throughput,
                actual: actual_throughput.unwrap(),
            }))))
        } else {
            poll_res
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        todo!()
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
    use std::time::{Duration, UNIX_EPOCH};

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
            unreachable!("body can't be read, so this won't be called")
        }
    }

    #[tokio::test()]
    async fn test_self_waking() {
        let (time_source, async_sleep) = instant_time_and_sleep(UNIX_EPOCH);
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

    // These tests use `hyper::body::Body::wrap_stream`
    #[cfg(feature = "connector-hyper-0-14-x")]
    mod connector_hyper_0_14_x {
        use super::*;
        use crate::client::http::body::minimum_throughput::throughput::Throughput;
        use aws_smithy_async::rt::sleep::AsyncSleep;
        use aws_smithy_async::test_util::{InstantSleep, ManualTimeSource};
        use aws_smithy_types::body::SdkBody;
        use aws_smithy_types::byte_stream::{AggregatedBytes, ByteStream};
        use aws_smithy_types::error::display::DisplayErrorContext;
        use bytes::Bytes;
        use once_cell::sync::Lazy;
        use pretty_assertions::assert_eq;
        use std::convert::Infallible;
        use std::future::Future;

        fn create_test_stream(
            async_sleep: impl AsyncSleep + Clone,
        ) -> impl futures_util::Stream<Item = Result<Bytes, Infallible>> {
            futures_util::stream::unfold(1, move |state| {
                let async_sleep = async_sleep.clone();
                async move {
                    if state > 255 {
                        None
                    } else {
                        async_sleep.sleep(Duration::from_secs(1)).await;
                        Some((
                            Result::<_, Infallible>::Ok(Bytes::from_static(b"00000000")),
                            state + 1,
                        ))
                    }
                }
            })
        }

        const EXPECTED_BYTES: Lazy<Vec<u8>> = Lazy::new(|| {
            (1..=255)
                .map(|_| b"00000000")
                .flatten()
                .map(|byte| *byte)
                .collect()
        });

        fn eight_byte_per_second_stream_with_minimum_throughput_timeout(
            minimum_throughput: (u64, Duration),
        ) -> (
            impl Future<Output = Result<AggregatedBytes, aws_smithy_types::byte_stream::error::Error>>,
            ManualTimeSource,
            InstantSleep,
        ) {
            let (time_source, async_sleep) = instant_time_and_sleep(UNIX_EPOCH);
            let time_clone = time_source.clone();

            // Will send ~8 bytes per second.
            let stream = create_test_stream(async_sleep.clone());
            let body = ByteStream::new(SdkBody::from(hyper::body::Body::wrap_stream(stream)));
            let body = body.map(move |body| {
                let time_source = time_clone.clone();
                // We don't want to log these sleeps because it would duplicate
                // the `sleep` calls being logged by the MTB
                let async_sleep = InstantSleep::unlogged();
                SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(
                    MinimumThroughputBody::new(time_source, async_sleep, body, minimum_throughput),
                ))
            });

            (body.collect(), time_source, async_sleep)
        }

        async fn expect_error(minimum_throughput: (u64, Duration)) {
            let (res, ..) =
                eight_byte_per_second_stream_with_minimum_throughput_timeout(minimum_throughput);
            let expected_err = Error::ThroughputBelowMinimum {
                expected: minimum_throughput.into(),
                actual: Throughput::new(8.889, Duration::from_secs(1)),
            };
            match res.await {
                Ok(_) => {
                    panic!("response succeeded instead of returning the expected error '{expected_err}'")
                }
                Err(actual_err) => {
                    assert_eq!(
                        expected_err.to_string(),
                        // We need to source this so that we don't get the streaming error it's wrapped in.
                        actual_err.source().unwrap().to_string()
                    );
                }
            }
        }

        #[tokio::test]
        async fn test_throughput_timeout_less_than() {
            let minimum_throughput = (9, Duration::from_secs(1));
            expect_error(minimum_throughput).await;
        }

        async fn expect_success(minimum_throughput: (u64, Duration)) {
            let (res, time_source, async_sleep) =
                eight_byte_per_second_stream_with_minimum_throughput_timeout(minimum_throughput);
            match res.await {
                Ok(res) => {
                    assert_eq!(255.0, time_source.seconds_since_unix_epoch());
                    assert_eq!(Duration::from_secs(255), async_sleep.total_duration());
                    assert_eq!(*EXPECTED_BYTES, res.to_vec());
                }
                Err(err) => panic!("{}", DisplayErrorContext(err.source().unwrap())),
            }
        }

        #[tokio::test]
        async fn test_throughput_timeout_equal_to() {
            let minimum_throughput = (32, Duration::from_secs(4));
            expect_success(minimum_throughput).await;
        }

        #[tokio::test]
        async fn test_throughput_timeout_greater_than() {
            let minimum_throughput = (20, Duration::from_secs(3));
            expect_success(minimum_throughput).await;
        }
    }
}
