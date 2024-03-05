/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A body-wrapping type that ensures data is being streamed faster than some lower limit.
//!
//! If data is being streamed too slowly, this body type will emit an error next time it's polled.

/// An implementation of v0.4 `http_body::Body` for `MinimumThroughputBody` and related code.
pub mod http_body_0_4_x;

/// Options for a [`MinimumThroughputBody`].
pub mod options;
pub use throughput::Throughput;
mod throughput;

use aws_smithy_async::rt::sleep::Sleep;
use aws_smithy_async::rt::sleep::{AsyncSleep, SharedAsyncSleep};
use aws_smithy_async::time::{SharedTimeSource, TimeSource};
use aws_smithy_runtime_api::{
    box_error::BoxError,
    client::{
        http::HttpConnectorFuture, result::ConnectorError, runtime_components::RuntimeComponents,
        stalled_stream_protection::StalledStreamProtectionConfig,
    },
};
use aws_smithy_runtime_api::{client::orchestrator::HttpResponse, shared::IntoShared};
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
use options::MinimumThroughputBodyOptions;
use std::{
    fmt,
    sync::{Arc, Mutex},
    task::Poll,
};
use std::{future::Future, pin::Pin};
use std::{
    task::Context,
    time::{Duration, SystemTime},
};
use throughput::ThroughputLogs;

pin_project_lite::pin_project! {
    /// A body-wrapping type that ensures data is being streamed faster than some lower limit.
    ///
    /// If data is being streamed too slowly, this body type will emit an error next time it's polled.
    pub struct MinimumThroughputBody<B> {
        async_sleep: SharedAsyncSleep,
        time_source: SharedTimeSource,
        options: MinimumThroughputBodyOptions,
        throughput_logs: ThroughputLogs,
        #[pin]
        sleep_fut: Option<Sleep>,
        #[pin]
        grace_period_fut: Option<Sleep>,
        #[pin]
        inner: B,
    }
}

const SIZE_OF_ONE_LOG: usize = std::mem::size_of::<(SystemTime, u64)>(); // 24 bytes per log
const NUMBER_OF_LOGS_IN_ONE_KB: f64 = 1024.0 / SIZE_OF_ONE_LOG as f64;

impl<B> MinimumThroughputBody<B> {
    /// Create a new minimum throughput body.
    pub fn new(
        time_source: impl TimeSource + 'static,
        async_sleep: impl AsyncSleep + 'static,
        body: B,
        options: MinimumThroughputBodyOptions,
    ) -> Self {
        Self {
            throughput_logs: ThroughputLogs::new(
                // Never keep more than 10KB of logs in memory. This currently
                // equates to 426 logs.
                (NUMBER_OF_LOGS_IN_ONE_KB * 10.0) as usize,
            ),
            async_sleep: async_sleep.into_shared(),
            time_source: time_source.into_shared(),
            inner: body,
            sleep_fut: None,
            grace_period_fut: None,
            options,
        }
    }
}

#[derive(Debug, PartialEq)]
enum Error {
    ThroughputBelowMinimum {
        expected: Throughput,
        actual: Throughput,
    },
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
        }
    }
}

impl std::error::Error for Error {}

/// Used to store the upload throughput in the interceptor context.
#[derive(Clone, Debug)]
pub(crate) struct UploadThroughput {
    log: Arc<Mutex<ThroughputLogs>>,
}

impl UploadThroughput {
    pub(crate) fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(ThroughputLogs::new(
                // Never keep more than 10KB of logs in memory. This currently
                // equates to 426 logs.
                (NUMBER_OF_LOGS_IN_ONE_KB * 10.0) as usize,
            ))),
        }
    }

    pub(crate) fn push(&self, now: SystemTime, bytes: u64) {
        self.log.lock().unwrap().push((now, bytes));
    }

    pub(crate) fn calculate_throughput(
        &self,
        now: SystemTime,
        time_window: Duration,
    ) -> Option<Throughput> {
        self.log
            .lock()
            .unwrap()
            .calculate_throughput(now, time_window)
    }
}

impl Storable for UploadThroughput {
    type Storer = StoreReplace<Self>;
}

pin_project_lite::pin_project! {
    pub(crate) struct ThroughputReadingBody<B> {
        time_source: SharedTimeSource,
        throughput: UploadThroughput,
        #[pin]
        inner: B,
    }
}

impl<B> ThroughputReadingBody<B> {
    pub(crate) fn new(
        time_source: SharedTimeSource,
        throughput: UploadThroughput,
        body: B,
    ) -> Self {
        Self {
            time_source,
            throughput,
            inner: body,
        }
    }
}

pin_project_lite::pin_project! {
    struct ThroughputCheckFuture {
        #[pin]
        response: HttpConnectorFuture,
        #[pin]
        check_interval: Option<Sleep>,
        #[pin]
        grace_period: Option<Sleep>,

        time_source: SharedTimeSource,
        sleep_impl: SharedAsyncSleep,
        upload_throughput: UploadThroughput,
        minimum_throughput: Throughput,
        time_window: Duration,
        grace_time: Duration,

        failing_throughput: Option<Throughput>,
    }
}

impl ThroughputCheckFuture {
    fn new(
        response: HttpConnectorFuture,
        time_source: SharedTimeSource,
        sleep_impl: SharedAsyncSleep,
        upload_throughput: UploadThroughput,
        minimum_throughput: Throughput,
        time_window: Duration,
        grace_time: Duration,
    ) -> Self {
        Self {
            response,
            check_interval: Some(sleep_impl.sleep(time_window)),
            grace_period: None,
            time_source,
            sleep_impl,
            upload_throughput,
            minimum_throughput,
            time_window,
            grace_time,
            failing_throughput: None,
        }
    }
}

impl Future for ThroughputCheckFuture {
    type Output = Result<HttpResponse, ConnectorError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        if let Poll::Ready(output) = this.response.poll(cx) {
            return Poll::Ready(output);
        } else {
            let mut below_minimum_throughput = false;
            let check_interval_expired = this
                .check_interval
                .as_mut()
                .as_pin_mut()
                .expect("always set")
                .poll(cx)
                .is_ready();
            if check_interval_expired {
                // Set up the next check interval
                *this.check_interval = Some(this.sleep_impl.sleep(*this.time_window));

                // Wake so that the check interval future gets polled
                // next time this poll method is called. If it never gets polled,
                // then this task won't be woken to check again.
                cx.waker().wake_by_ref();
            }

            let should_check = check_interval_expired || this.grace_period.is_some();
            if should_check {
                let now = this.time_source.now();
                let current_throughput = this
                    .upload_throughput
                    .calculate_throughput(now, *this.time_window);
                below_minimum_throughput = current_throughput
                    .as_ref()
                    .map(|tp| tp < this.minimum_throughput)
                    .unwrap_or_default();
                tracing::debug!("current throughput: {current_throughput:?}, below minimum: {below_minimum_throughput}");
                if below_minimum_throughput && !this.failing_throughput.is_some() {
                    *this.failing_throughput = current_throughput;
                } else if !below_minimum_throughput {
                    *this.failing_throughput = None;
                }
            }

            // If we kicked off a grace period and are now satisfied, clear out the grace period
            if !below_minimum_throughput && this.grace_period.is_some() {
                tracing::debug!("upload minimum throughput recovered during grace period");
                *this.grace_period = None;
            }
            if below_minimum_throughput {
                // Start a grace period if below minimum throughput
                if this.grace_period.is_none() {
                    tracing::debug!(
                        grace_period=?*this.grace_time,
                        "upload minimum throughput below configured minimum; starting grace period"
                    );
                    *this.grace_period = Some(this.sleep_impl.sleep(*this.grace_time));
                }
                // Check the grace period if one is already set and we're not satisfied
                if let Some(grace_period) = this.grace_period.as_pin_mut() {
                    if grace_period.poll(cx).is_ready() {
                        tracing::debug!("grace period ended; timing out request");
                        return Poll::Ready(Err(ConnectorError::timeout(
                            Error::ThroughputBelowMinimum {
                                expected: *this.minimum_throughput,
                                actual: this
                                    .failing_throughput
                                    .expect("always set if there's a grace period"),
                            }
                            .into(),
                        )));
                    }
                }
            }
        }
        Poll::Pending
    }
}

pin_project_lite::pin_project! {
    #[project = EnumProj]
    pub(crate) enum MaybeThroughputCheckFuture {
        Direct { #[pin] future: HttpConnectorFuture },
        Checked { #[pin] future: ThroughputCheckFuture },
    }
}

impl MaybeThroughputCheckFuture {
    pub(crate) fn new(
        cfg: &mut ConfigBag,
        components: &RuntimeComponents,
        connector_future: HttpConnectorFuture,
    ) -> Self {
        if let Some(sspcfg) = cfg.load::<StalledStreamProtectionConfig>().cloned() {
            if sspcfg.is_enabled() {
                let options = MinimumThroughputBodyOptions::from(sspcfg);
                return Self::new_inner(
                    connector_future,
                    components.time_source(),
                    components.sleep_impl(),
                    cfg.interceptor_state().load::<UploadThroughput>().cloned(),
                    Some(options),
                );
            }
        }
        tracing::debug!("no minimum upload throughput checks");
        Self::new_inner(connector_future, None, None, None, None)
    }

    fn new_inner(
        response: HttpConnectorFuture,
        time_source: Option<SharedTimeSource>,
        sleep_impl: Option<SharedAsyncSleep>,
        upload_throughput: Option<UploadThroughput>,
        options: Option<MinimumThroughputBodyOptions>,
    ) -> Self {
        match (time_source, sleep_impl, upload_throughput, options) {
            (Some(time_source), Some(sleep_impl), Some(upload_throughput), Some(options)) => {
                tracing::debug!(options=?options, "applying minimum upload throughput check future");
                Self::Checked {
                    future: ThroughputCheckFuture::new(
                        response,
                        time_source,
                        sleep_impl,
                        upload_throughput,
                        options.minimum_throughput(),
                        options.check_window(),
                        options.grace_period(),
                    ),
                }
            }
            _ => Self::Direct { future: response },
        }
    }
}

impl Future for MaybeThroughputCheckFuture {
    type Output = Result<HttpResponse, ConnectorError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EnumProj::Direct { future } => future.poll(cx),
            EnumProj::Checked { future } => future.poll(cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{assert_str_contains, test_util::capture_test_logs::capture_test_logs};
    use aws_smithy_async::test_util::tick_advance_sleep::tick_advance_time_and_sleep;
    use aws_smithy_types::{body::SdkBody, error::display::DisplayErrorContext};
    use std::future::IntoFuture;

    const TEST_TIME_WINDOW: Duration = Duration::from_secs(1);

    #[tokio::test]
    async fn throughput_check_constant_rate_success() {
        let minimum_throughput = Throughput::new_bytes_per_second(990);
        let grace_time = Duration::from_secs(1);
        let actual_throughput_bps = 1000.0;
        let transfer_fn = |_| actual_throughput_bps * TEST_TIME_WINDOW.as_secs_f64();
        let result = throughput_check_test(minimum_throughput, grace_time, transfer_fn).await;
        let response = result.expect("no timeout");
        assert_eq!(200, response.status().as_u16());
    }

    #[tokio::test]
    async fn throughput_check_constant_rate_timeout() {
        let minimum_throughput = Throughput::new_bytes_per_second(1100);
        let grace_time = Duration::from_secs(1);
        let actual_throughput_bps = 1000.0;
        let transfer_fn = |_| actual_throughput_bps * TEST_TIME_WINDOW.as_secs_f64();
        let result = throughput_check_test(minimum_throughput, grace_time, transfer_fn).await;
        let error = result.err().expect("times out");
        assert_str_contains!(
            DisplayErrorContext(&error).to_string(),
            "minimum throughput was specified at 1100 B/s, but throughput of 1000 B/s was observed"
        );
    }

    #[tokio::test]
    async fn throughput_check_grace_time_recovery() {
        let minimum_throughput = Throughput::new_bytes_per_second(1000);
        let grace_time = Duration::from_secs(3);
        let actual_throughput_bps = 1000.0;
        let transfer_fn = |window| {
            if window <= 5 || window > 7 {
                actual_throughput_bps * TEST_TIME_WINDOW.as_secs_f64()
            } else {
                0.0
            }
        };
        let result = throughput_check_test(minimum_throughput, grace_time, transfer_fn).await;
        let response = result.expect("no timeout");
        assert_eq!(200, response.status().as_u16());
    }

    async fn throughput_check_test<F>(
        minimum_throughput: Throughput,
        grace_time: Duration,
        transfer_fn: F,
    ) -> Result<HttpResponse, ConnectorError>
    where
        F: Fn(u64) -> f64,
    {
        let _logs = capture_test_logs();
        let (time_source, sleep_impl) = tick_advance_time_and_sleep();

        let response = HttpResponse::try_from(
            http::Response::builder()
                .status(200)
                .body(SdkBody::empty())
                .unwrap(),
        )
        .unwrap();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let upload_throughput = UploadThroughput::new();
        let check_task = tokio::spawn(ThroughputCheckFuture::new(
            HttpConnectorFuture::new(async move { Ok(response_rx.into_future().await.unwrap()) }),
            time_source.clone().into_shared(),
            sleep_impl.into_shared(),
            upload_throughput.clone(),
            minimum_throughput,
            TEST_TIME_WINDOW,
            grace_time,
        ));

        // simulate 20 check time windows at `actual_throughput` bytes/sec
        for window in 0..20 {
            let bytes = (transfer_fn)(window);
            upload_throughput.push(time_source.now(), (bytes + 0.5) as u64);
            time_source.tick(TEST_TIME_WINDOW).await;
            println!("window {window}");
        }
        let _ = response_tx.send(response);
        println!("upload finished");

        check_task.await.expect("no panic")
    }
}
