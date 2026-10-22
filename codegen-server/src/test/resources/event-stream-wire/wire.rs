/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::event_stream_message_timeout::MessageTimeoutError;
use crate::input::{
    GlobalIdleEventsInput, HealthInput, NoIdleEventsInput, NoInitialEventsInput,
    PublishEventsInput, RetryEventsInput,
};
use crate::output::{
    GlobalIdleEventsOutput, HealthOutput, NoIdleEventsOutput, NoInitialEventsOutput,
    PublishEventsOutput, RetryEventsOutput,
};
use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
use bytes::Bytes;
use http_body_util::BodyExt;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout, Instant};

static HANDLER_CALLS: AtomicUsize = AtomicUsize::new(0);
static RESULTS: Mutex<Vec<(i32, String)>> = Mutex::new(Vec::new());

struct AbortTask<T = ()>(JoinHandle<T>);

impl<T> Drop for AbortTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn events(
    mut stream: aws_smithy_http::event_stream::Receiver<
        crate::model::Events,
        aws_smithy_http::event_stream::MessageStreamError,
    >,
    retry: bool,
) -> (i32, String) {
    HANDLER_CALLS.fetch_add(1, Ordering::Release);
    let mut count = 0;
    let mut retried = false;
    let outcome = loop {
        match stream.recv().await {
            Ok(Some(event)) => {
                assert_eq!(event.as_event().unwrap().data(), Some("hello"));
                count += 1;
            }
            Ok(None) => break if retried { "eof-after-timeout" } else { "eof" },
            Err(error) => {
                let error = MessageTimeoutError::find_in_chain(&error)
                    .unwrap_or_else(|| panic!("unexpected stream error: {error:?}"));
                let outcome = match error {
                    MessageTimeoutError::InitialMessage { .. } => {
                        panic!("initial expiry must stay before the handler")
                    }
                    MessageTimeoutError::MessageCompletion { .. } => "completion",
                    MessageTimeoutError::Idle { .. } => "idle",
                };
                if retry && !retried {
                    retried = true;
                    continue;
                }
                break outcome;
            }
        }
    };
    let result = (count, outcome.to_owned());
    RESULTS.lock().unwrap().push(result.clone());
    result
}

async fn health(_input: HealthInput) -> HealthOutput {
    HealthOutput {}
}

async fn start_server() -> (SocketAddr, AbortTask) {
    let config = crate::WireServiceConfig::builder().build();
    let app = crate::WireService::builder(config)
        .publish_events(|input: PublishEventsInput| async move {
            let (count, outcome) = events(input.events, false).await;
            Ok::<_, crate::error::PublishEventsError>(PublishEventsOutput { count, outcome })
        })
        .retry_events(|input: RetryEventsInput| async move {
            let (count, outcome) = events(input.events, true).await;
            Ok::<_, crate::error::RetryEventsError>(RetryEventsOutput { count, outcome })
        })
        .no_idle_events(|input: NoIdleEventsInput| async move {
            let (count, outcome) = events(input.events, false).await;
            Ok::<_, crate::error::NoIdleEventsError>(NoIdleEventsOutput { count, outcome })
        })
        .global_idle_events(|input: GlobalIdleEventsInput| async move {
            let (count, outcome) = events(input.events, false).await;
            Ok::<_, crate::error::GlobalIdleEventsError>(GlobalIdleEventsOutput { count, outcome })
        })
        .no_initial_events(|input: NoInitialEventsInput| async move {
            let (count, outcome) = events(input.events, false).await;
            Ok::<_, crate::error::NoInitialEventsError>(NoInitialEventsOutput { count, outcome })
        })
        .health(health)
        .build_unchecked();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = AbortTask(tokio::spawn(async move {
        crate::serve(listener, app.into_make_service())
            .max_connections(1)
            .configure_hyper(|builder| builder.http1_only())
            .await
            .unwrap();
    }));
    (addr, task)
}

#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)] // Each generated protocol crate instantiates one variant.
enum WireProtocol {
    Rest,
    Json,
    Cbor,
}

impl WireProtocol {
    fn name(self) -> &'static str {
        match self {
            Self::Rest => "rest",
            Self::Json => "json",
            Self::Cbor => "cbor",
        }
    }

    fn frame(self, initial: bool) -> Bytes {
        let payload: &'static [u8] = match (self, initial) {
            (Self::Cbor, true) => &[0xa0],
            (Self::Cbor, false) => b"\xa1\x64data\x65hello",
            (_, true) => b"{}",
            (_, false) => br#"{"data":"hello"}"#,
        };
        let message = Message::new(Bytes::from_static(payload))
            .add_header(Header::new(
                ":message-type",
                HeaderValue::String("event".into()),
            ))
            .add_header(Header::new(
                ":event-type",
                HeaderValue::String(if initial { "initial-request" } else { "event" }.into()),
            ))
            .add_header(Header::new(
                ":content-type",
                HeaderValue::String(
                    if self == Self::Cbor {
                        "application/cbor"
                    } else {
                        "application/json"
                    }
                    .into(),
                ),
            ));
        let mut bytes = Vec::new();
        aws_smithy_eventstream::frame::write_message_to(&message, &mut bytes).unwrap();
        bytes.into()
    }
}

// Like ManualEventStreamClient, use a channel-backed Hyper body to control actual
// request bytes. A delayed connector only delays connection establishment; it cannot
// exercise a frame that stalls after its first bytes have reached the server.
struct ChannelBody(mpsc::Receiver<Bytes>);

impl http_body_1x::Body for ChannelBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body_1x::Frame<Bytes>, Self::Error>>> {
        self.0
            .poll_recv(cx)
            .map(|data| data.map(|data| Ok(http_body_1x::Frame::data(data))))
    }
}

struct WireClient {
    body: Option<mpsc::Sender<Bytes>>,
    response: JoinHandle<http_1x::StatusCode>,
    connection: AbortTask<Result<(), hyper::Error>>,
}

impl Drop for WireClient {
    fn drop(&mut self) {
        self.response.abort();
    }
}

impl WireClient {
    async fn connect(
        addr: SocketAddr,
        protocol: WireProtocol,
        operation: &str,
        streaming: bool,
    ) -> Self {
        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.set_nodelay(true).unwrap();
        let io = hyper_util::rt::TokioIo::new(stream);
        let (mut sender, connection) = hyper::client::conn::http1::handshake::<_, ChannelBody>(io)
            .await
            .unwrap();
        let connection = AbortTask(tokio::spawn(connection));
        let (body, receiver) = mpsc::channel(8);
        let uri = match protocol {
            WireProtocol::Rest => format!("/{operation}"),
            WireProtocol::Json => "/".into(),
            WireProtocol::Cbor => format!("/service/WireService/operation/{operation}"),
        };
        let mut request = http_1x::Request::builder()
            .method("POST")
            .uri(uri)
            .header("Host", "localhost")
            .header("Connection", "close")
            .header(
                "Content-Type",
                if streaming {
                    "application/vnd.amazon.eventstream"
                } else {
                    match protocol {
                        WireProtocol::Rest => "application/json",
                        WireProtocol::Json => "application/x-amz-json-1.1",
                        WireProtocol::Cbor => "application/cbor",
                    }
                },
            );
        match protocol {
            WireProtocol::Rest => {}
            WireProtocol::Json => {
                request = request.header("x-amz-target", format!("WireService.{operation}"));
            }
            WireProtocol::Cbor => {
                request = request.header("smithy-protocol", "rpc-v2-cbor");
            }
        }
        let request = request.body(ChannelBody(receiver)).unwrap();
        let response = tokio::spawn(async move {
            let response = sender.send_request(request).await.unwrap();
            let status = response.status();
            response.into_body().collect().await.unwrap();
            status
        });
        let client = Self {
            body: Some(body),
            response,
            connection,
        };
        if !streaming {
            client
                .send(Bytes::from_static(if protocol == WireProtocol::Cbor {
                    &[0xa0]
                } else {
                    b"{}"
                }))
                .await;
        }
        client
    }

    async fn send(&self, bytes: Bytes) {
        self.body.as_ref().unwrap().send(bytes).await.unwrap();
    }

    async fn initial(&self, protocol: WireProtocol) {
        if protocol != WireProtocol::Rest {
            self.send(protocol.frame(true)).await;
        }
    }

    fn finish(&mut self) {
        self.body.take();
    }

    async fn expect(mut self, outcome: Option<(&str, i32)>, rejection: bool) {
        let status = (&mut self.response).await.unwrap();
        // The Hyper connection driver finishes only when the server closes TCP.
        if let Err(error) = (&mut self.connection.0).await.unwrap() {
            // A timeout response closes TCP while the request body is deliberately
            // unfinished. Hyper reports that upload as incomplete even though the
            // complete response above was received and validated.
            assert!(
                self.body.is_some() && error.is_incomplete_message(),
                "unexpected HTTP connection failure: {error:?}"
            );
        }
        if rejection {
            assert_eq!(
                status,
                http_1x::StatusCode::REQUEST_TIMEOUT,
                "an initial-message timeout must be a retryable 408"
            );
            assert!(
                RESULTS.lock().unwrap().is_empty(),
                "rejection invoked the handler"
            );
        } else {
            assert!(status.is_success(), "{status}");
            if let Some((outcome, count)) = outcome {
                let results = std::mem::take(&mut *RESULTS.lock().unwrap());
                assert_eq!(results, vec![(count, outcome.to_owned())]);
            }
        }
    }

    async fn expect_after(
        self,
        start: Instant,
        duration: Duration,
        outcome: Option<(&str, i32)>,
        rejection: bool,
    ) {
        self.expect(outcome, rejection).await;
        let elapsed = start.elapsed();
        assert!(
            elapsed >= duration.saturating_sub(Duration::from_millis(80)),
            "{elapsed:?}"
        );
        assert!(elapsed <= duration + Duration::from_secs(1), "{elapsed:?}");
    }

    async fn pending_for(&mut self, duration: Duration) {
        assert!(
            timeout(duration, &mut self.response).await.is_err(),
            "request ended too early"
        );
    }
}

async fn wire_scenarios(protocol: WireProtocol) {
    let (addr, _server) = start_server().await;
    let start = Instant::now();
    let ms = Duration::from_millis;
    let mut passed = 0;
    let mut pass = |name: &str| {
        passed += 1;
        println!("PASS {} {name}", protocol.name());
    };
    if protocol == WireProtocol::Cbor {
        for partial in [false, true] {
            let calls = HANDLER_CALLS.load(Ordering::Acquire);
            let client = WireClient::connect(addr, protocol, "PublishEvents", true).await;
            let start = Instant::now();
            if partial {
                sleep(ms(200)).await;
                client.send(protocol.frame(true).slice(..2)).await;
            }
            client.expect_after(start, ms(300), None, true).await;
            assert_eq!(HANDLER_CALLS.load(Ordering::Acquire), calls);
            pass(if partial {
                "initial_partial_rejected_before_handler"
            } else {
                "initial_no_bytes_rejected_before_handler"
            });
        }
    }
    if protocol != WireProtocol::Rest {
        let calls = HANDLER_CALLS.load(Ordering::Acquire);
        let client = WireClient::connect(addr, protocol, "NoInitialEvents", true).await;
        let started = Instant::now();
        client.expect_after(started, ms(300), None, true).await;
        assert_eq!(HANDLER_CALLS.load(Ordering::Acquire), calls);
        pass("stream_only_RPC_missing_first_frame_rejected_before_handler");

        let client = WireClient::connect(addr, protocol, "NoInitialEvents", true).await;
        let started = Instant::now();
        client.send(protocol.frame(false).slice(..2)).await;
        client.expect_after(started, ms(300), None, true).await;
        assert_eq!(HANDLER_CALLS.load(Ordering::Acquire), calls);
        pass("stream_only_RPC_partial_first_frame_uses_initial_deadline");

        let mut client = WireClient::connect(addr, protocol, "NoInitialEvents", true).await;
        client.pending_for(ms(250)).await;
        assert_eq!(HANDLER_CALLS.load(Ordering::Acquire), calls);
        client.send(protocol.frame(false)).await;
        client.finish();
        client.expect(Some(("eof", 1)), false).await;
        assert_eq!(HANDLER_CALLS.load(Ordering::Acquire), calls + 1);
        pass("stream_only_RPC_first_event_is_buffered_before_handler");
    }
    let client = WireClient::connect(addr, protocol, "PublishEvents", true).await;
    let started = Instant::now();
    client.initial(protocol).await;
    client
        .expect_after(started, ms(400), Some(("idle", 0)), false)
        .await;
    pass("first_event_idle_error_reaches_handler");

    let client = WireClient::connect(addr, protocol, "PublishEvents", true).await;
    client.initial(protocol).await;
    client.send(protocol.frame(false)).await;
    client
        .expect_after(Instant::now(), ms(400), Some(("idle", 1)), false)
        .await;
    pass("idle_after_valid_event_reaches_handler_and_closes_TCP");

    let client = WireClient::connect(addr, protocol, "PublishEvents", true).await;
    client.initial(protocol).await;
    let frame = protocol.frame(false);
    let started = Instant::now();
    for offset in 0..3 {
        client.send(frame.slice(offset..offset + 1)).await;
        sleep(ms(60)).await;
    }
    client
        .expect_after(started, ms(200), Some(("completion", 0)), false)
        .await;
    pass("slow_drip_completion_error_reaches_handler");

    let mut client = WireClient::connect(addr, protocol, "PublishEvents", true).await;
    client.initial(protocol).await;
    for _ in 0..3 {
        client.send(protocol.frame(false)).await;
        sleep(ms(250)).await;
    }
    client.finish();
    client.expect(Some(("eof", 3)), false).await;
    pass("timely_events_do_not_conflict_with_other_deadlines");

    for operation in ["NoIdleEvents", "GlobalIdleEvents"] {
        let mut client = WireClient::connect(addr, protocol, operation, true).await;
        client.initial(protocol).await;
        client.pending_for(ms(650)).await;
        client.send(protocol.frame(false)).await;
        client.finish();
        client.expect(Some(("eof", 1)), false).await;
        pass(&format!(
            "{operation}_remains_open_beyond_short_idle_override"
        ));
    }
    let client = WireClient::connect(addr, protocol, "GlobalIdleEvents", true).await;
    let started = Instant::now();
    client.initial(protocol).await;
    client
        .expect_after(started, ms(1000), Some(("idle", 0)), false)
        .await;
    pass("inherited_global_idle_deadline_reaches_handler");

    for partial in [false, true] {
        let mut client = WireClient::connect(addr, protocol, "RetryEvents", true).await;
        client.initial(protocol).await;
        let frame = protocol.frame(false);
        if partial {
            client.send(frame.slice(..2)).await;
        }
        client
            .pending_for(ms(if partial { 300 } else { 600 }))
            .await;
        client
            .send(if partial { frame.slice(2..) } else { frame })
            .await;
        client.finish();
        client.expect(Some(("eof-after-timeout", 1)), false).await;
        pass(if partial {
            "handler_can_retry_completion_and_receive_event"
        } else {
            "handler_can_retry_idle_and_receive_event"
        });
    }
    let first = WireClient::connect(addr, protocol, "PublishEvents", true).await;
    first.initial(protocol).await;
    sleep(ms(50)).await;
    let mut second = WireClient::connect(addr, protocol, "Health", false).await;
    second.finish();
    second.pending_for(ms(100)).await;
    first.expect(Some(("idle", 0)), false).await;
    second.expect(None, false).await;
    pass("idle_expiry_releases_connection_slot_and_server_still_serves_requests");
    assert_eq!(
        passed,
        match protocol {
            WireProtocol::Rest => 10,
            WireProtocol::Json => 13,
            WireProtocol::Cbor => 15,
        }
    );
    println!(
        "PASS {} all {passed} TCP scenarios in {:?}",
        protocol.name(),
        start.elapsed()
    );
}
