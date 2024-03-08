/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_async::time::TimeSource;
use aws_smithy_runtime::{assert_str_contains, test_util::capture_test_logs::capture_test_logs};
use aws_smithy_types::error::display::DisplayErrorContext;
use bytes::Bytes;
use std::time::Duration;
use tracing::info;

/// No really, it's 42 bytes long... super neat
const NEAT_DATA: Bytes = Bytes::from_static(b"some really neat data");

/// Ticks time forward by the given duration, and logs the current time for debugging.
macro_rules! tick {
    ($ticker:ident, $duration:expr) => {
        $ticker.tick($duration).await;
        let now = $ticker
            .now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap();
        tracing::info!("ticked {:?}, now at {:?}", $duration, now);
    };
}

/// Scenario: Successful upload at a rate above the minimum throughput.
/// Expected: MUST NOT timeout.
#[tokio::test]
async fn upload_success() {
    let _logs = capture_test_logs();

    let (server, time, sleep) = eager_server(true);
    let op = operation(server, time, sleep);

    let (body, body_sender) = channel_body();
    let result = tokio::spawn(async move { op.invoke(body).await });

    for _ in 0..100 {
        body_sender.send(NEAT_DATA).await.unwrap();
    }
    drop(body_sender);

    assert_eq!(200, result.await.unwrap().expect("success").as_u16());
}

/// Scenario: Upload takes some time to start, but then goes normally.
/// Expected: MUST NOT timeout.
#[tokio::test]
async fn upload_slow_start() {
    let _logs = capture_test_logs();

    let (server, time, sleep) = eager_server(false);
    let op = operation(server, time.clone(), sleep);

    let (body, body_sender) = channel_body();
    let result = tokio::spawn(async move { op.invoke(body).await });

    let _streamer = tokio::spawn(async move {
        // Advance longer than the grace period. This shouldn't fail since
        // it is the customer's side that hasn't produced data yet, not a server issue.
        time.tick(Duration::from_secs(10)).await;

        for _ in 0..100 {
            body_sender.send(NEAT_DATA).await.unwrap();
            time.tick(Duration::from_secs(1)).await;
        }
        drop(body_sender);
        time.tick(Duration::from_secs(1)).await;
    });

    assert_eq!(200, result.await.unwrap().expect("success").as_u16());
}

/// Scenario: The upload is going fine, but falls below the minimum throughput.
/// Expected: MUST timeout.
#[tokio::test]
async fn upload_too_slow() {
    let _logs = capture_test_logs();

    // Server that starts off fast enough, but gets slower over time until it should timeout.
    let (server, time, sleep) = time_sequence_server([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    let op = operation(server, time, sleep);

    let (body, body_sender) = channel_body();
    let result = tokio::spawn(async move { op.invoke(body).await });

    let _streamer = tokio::spawn(async move {
        for send in 0..100 {
            info!("send {send}");
            body_sender.send(NEAT_DATA).await.unwrap();
        }
        drop(body_sender);
    });

    let err = result
        .await
        .expect("no panics")
        .expect_err("should have timed out");
    assert_str_contains!(
        DisplayErrorContext(&err).to_string(),
        "minimum throughput was specified at 1 B/s, but throughput of 0 B/s was observed"
    );
}

/// Scenario: The server stops asking for data, the client maxes out its send buffer,
///           and the request stream stops being polled.
/// Expected: MUST timeout after the grace period completes.
#[tokio::test]
async fn upload_stalls() {
    let _logs = capture_test_logs();

    let (server, time, sleep) = stalling_server();
    let op = operation(server, time.clone(), sleep);

    let (body, body_sender) = channel_body();
    let result = tokio::spawn(async move { op.invoke(body).await });

    let _streamer = tokio::spawn(async move {
        for send in 1..=100 {
            info!("send {send}");
            body_sender.send(NEAT_DATA).await.unwrap();
            tick!(time, Duration::from_secs(1));
        }
        drop(body_sender);
        time.tick(Duration::from_secs(1)).await;
    });

    let err = result
        .await
        .expect("no panics")
        .expect_err("should have timed out");
    assert_str_contains!(
        DisplayErrorContext(&err).to_string(),
        "minimum throughput was specified at 1 B/s, but throughput of 0 B/s was observed"
    );
}

// Scenario: The server stops asking for data, the client maxes out its send buffer,
//           and the request stream stops being polled. However, before the grace period
//           is over, the server recovers and starts asking for data again.
// Expected: MUST NOT timeout.
#[tokio::test]
async fn upload_stall_recovery_in_grace_period() {
    let _logs = capture_test_logs();

    // Server starts off fast enough, but then slows down almost up to
    // the grace period, and then recovers.
    let (server, time, sleep) = time_sequence_server([1, 4, 1]);
    let op = operation(server, time, sleep);

    let (body, body_sender) = channel_body();
    let result = tokio::spawn(async move { op.invoke(body).await });

    let _streamer = tokio::spawn(async move {
        for send in 0..100 {
            info!("send {send}");
            body_sender.send(NEAT_DATA).await.unwrap();
        }
        drop(body_sender);
    });

    assert_eq!(200, result.await.unwrap().expect("success").as_u16());
}

// Scenario: The customer isn't providing data on the stream fast enough to satisfy
//           the minimum throughput. This shouldn't be considered a stall since the
//           server is asking for more data and could handle it if it were available.
// Expected: MUST NOT timeout.
#[tokio::test]
async fn user_provides_data_too_slowly() {
    let _logs = capture_test_logs();

    let (server, time, sleep) = eager_server(false);
    let op = operation(server, time.clone(), sleep.clone());

    let (body, body_sender) = channel_body();
    let result = tokio::spawn(async move { op.invoke(body).await });

    let _streamer = tokio::spawn(async move {
        body_sender.send(NEAT_DATA).await.unwrap();
        tick!(time, Duration::from_secs(1));
        body_sender.send(NEAT_DATA).await.unwrap();

        // Now advance 10 seconds before sending more data, simulating a
        // customer taking time to produce more data to stream.
        tick!(time, Duration::from_secs(10));
        body_sender.send(NEAT_DATA).await.unwrap();
        drop(body_sender);
        tick!(time, Duration::from_secs(1));
    });

    assert_eq!(200, result.await.unwrap().expect("success").as_u16());
}

use test_tools::*;
mod test_tools {
    use aws_smithy_async::test_util::tick_advance_sleep::{
        tick_advance_time_and_sleep, TickAdvanceSleep, TickAdvanceTime,
    };
    use aws_smithy_async::time::TimeSource;
    use aws_smithy_runtime::client::{
        orchestrator::operation::Operation,
        stalled_stream_protection::{
            StalledStreamProtectionInterceptor, StalledStreamProtectionInterceptorKind,
        },
    };
    use aws_smithy_runtime_api::{
        client::{
            http::{
                HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings,
                SharedHttpConnector,
            },
            orchestrator::{HttpRequest, HttpResponse},
            runtime_components::RuntimeComponents,
            stalled_stream_protection::StalledStreamProtectionConfig,
        },
        http::StatusCode,
        shared::IntoShared,
    };
    use aws_smithy_types::{body::SdkBody, timeout::TimeoutConfig};
    use bytes::Bytes;
    use http_body_0_4::Body;
    use pin_utils::pin_mut;
    use std::{
        collections::VecDeque,
        convert::Infallible,
        future::poll_fn,
        mem,
        pin::Pin,
        task::{Context, Poll},
        time::Duration,
    };
    use tracing::instrument::Instrument;

    #[derive(Debug)]
    struct FakeServer(SharedHttpConnector);

    impl HttpClient for FakeServer {
        fn http_connector(
            &self,
            _settings: &HttpConnectorSettings,
            _components: &RuntimeComponents,
        ) -> SharedHttpConnector {
            self.0.clone()
        }
    }

    struct ChannelBody {
        receiver: tokio::sync::mpsc::Receiver<Bytes>,
    }
    impl http_body_0_4::Body for ChannelBody {
        type Data = Bytes;
        type Error = Infallible;

        fn poll_data(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
            match self.receiver.poll_recv(cx) {
                Poll::Ready(value) => Poll::Ready(value.map(|v| Ok(v))),
                Poll::Pending => Poll::Pending,
            }
        }

        fn poll_trailers(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
            unreachable!()
        }
    }

    pub fn channel_body() -> (SdkBody, tokio::sync::mpsc::Sender<Bytes>) {
        let (sender, receiver) = tokio::sync::mpsc::channel(1000);
        (SdkBody::from_body_0_4(ChannelBody { receiver }), sender)
    }

    pub fn successful_response() -> HttpResponse {
        HttpResponse::try_from(
            http::Response::builder()
                .status(200)
                .body(SdkBody::empty())
                .unwrap(),
        )
        .unwrap()
    }

    pub fn operation(
        http_connector: impl HttpConnector + 'static,
        time: TickAdvanceTime,
        sleep: TickAdvanceSleep,
    ) -> Operation<SdkBody, StatusCode, Infallible> {
        let operation = Operation::builder()
            .service_name("test")
            .operation_name("test")
            .http_client(FakeServer(http_connector.into_shared()))
            .endpoint_url("http://localhost:1234/doesntmatter")
            .no_auth()
            .no_retry()
            .timeout_config(TimeoutConfig::disabled())
            .serializer(|body: SdkBody| Ok(HttpRequest::new(body)))
            .deserializer::<_, Infallible>(|response| Ok(response.status()))
            .stalled_stream_protection(
                StalledStreamProtectionConfig::enabled()
                    .grace_period(Duration::from_secs(5))
                    .build(),
            )
            .interceptor(StalledStreamProtectionInterceptor::new(
                StalledStreamProtectionInterceptorKind::RequestAndResponseBody,
            ))
            .sleep_impl(sleep)
            .time_source(time)
            .build();
        operation
    }

    /// Creates a fake HttpConnector implementation that calls the given async $body_fn
    /// to get the response body. This $body_fn is given a request body, time, and sleep.
    macro_rules! fake_server {
        ($name:ident, $body_fn:expr) => {
            fake_server!($name, $body_fn, (), ())
        };
        ($name:ident, $body_fn:expr, $params_ty:ty, $params:expr) => {{
            #[derive(Debug)]
            struct $name(TickAdvanceTime, TickAdvanceSleep, $params_ty);
            impl HttpConnector for $name {
                fn call(&self, mut request: HttpRequest) -> HttpConnectorFuture {
                    let time = self.0.clone();
                    let sleep = self.1.clone();
                    let params = self.2.clone();
                    let span = tracing::span!(tracing::Level::INFO, "FAKE SERVER");
                    HttpConnectorFuture::new(
                        async move {
                            let mut body = SdkBody::taken();
                            mem::swap(request.body_mut(), &mut body);
                            pin_mut!(body);

                            Ok($body_fn(body, time, sleep, params).await)
                        }
                        .instrument(span),
                    )
                }
            }
            let (time, sleep) = tick_advance_time_and_sleep();
            (
                $name(time.clone(), sleep.clone(), $params).into_shared(),
                time,
                sleep,
            )
        }};
    }

    /// Fake server/connector that immediately reads all incoming data with an
    /// optional 1 second gap in between polls.
    pub fn eager_server(
        advance_time: bool,
    ) -> (SharedHttpConnector, TickAdvanceTime, TickAdvanceSleep) {
        async fn fake_server(
            mut body: Pin<&mut SdkBody>,
            time: TickAdvanceTime,
            _: TickAdvanceSleep,
            advance_time: bool,
        ) -> HttpResponse {
            while poll_fn(|cx| body.as_mut().poll_data(cx)).await.is_some() {
                if advance_time {
                    tick!(time, Duration::from_secs(1));
                }
            }
            successful_response()
        }
        fake_server!(FakeServerConnector, fake_server, bool, advance_time)
    }

    /// Fake server/connector that reads some data, and then stalls.
    pub fn stalling_server() -> (SharedHttpConnector, TickAdvanceTime, TickAdvanceSleep) {
        async fn fake_server(
            mut body: Pin<&mut SdkBody>,
            _time: TickAdvanceTime,
            _sleep: TickAdvanceSleep,
            _: (),
        ) -> HttpResponse {
            let mut times = 5;
            while poll_fn(|cx| body.as_mut().poll_data(cx)).await.is_some() {
                times -= 1;
                if times <= 0 {
                    // never awake after this
                    tracing::info!("stalling indefinitely");
                    std::future::pending().await
                }
            }
            unreachable!()
        }
        fake_server!(FakeServerConnector, fake_server)
    }

    /// Fake server/connector that polls data after each period of time in the given
    /// sequence. Once the sequence completes, it will delay 1 second after each poll.
    pub fn time_sequence_server(
        time_sequence: impl IntoIterator<Item = u64>,
    ) -> (SharedHttpConnector, TickAdvanceTime, TickAdvanceSleep) {
        async fn fake_server(
            mut body: Pin<&mut SdkBody>,
            time: TickAdvanceTime,
            _sleep: TickAdvanceSleep,
            time_sequence: Vec<u64>,
        ) -> HttpResponse {
            let mut time_sequence: VecDeque<Duration> =
                time_sequence.into_iter().map(Duration::from_secs).collect();
            while poll_fn(|cx| body.as_mut().poll_data(cx)).await.is_some() {
                let next_time = time_sequence.pop_front().unwrap_or(Duration::from_secs(1));
                tick!(time, next_time);
            }
            successful_response()
        }
        fake_server!(
            FakeServerConnector,
            fake_server,
            Vec<u64>,
            time_sequence.into_iter().collect()
        )
    }
}
