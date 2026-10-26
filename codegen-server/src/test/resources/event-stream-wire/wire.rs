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
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Instant};

// Each scenario owns its observations, so assertions cannot consume another
// scenario's handler results.
struct HandlerState {
    calls: AtomicUsize,
    results: Mutex<Vec<(i32, String)>>,
    observations: mpsc::UnboundedSender<HandlerObservation>,
    resume_retry: Notify,
    finish: Notify,
    hold_completion: bool,
}

#[derive(Debug, PartialEq)]
enum HandlerObservation {
    Entered,
    Event(i32),
    Timeout(&'static str, Duration),
}

struct WireServer {
    addr: SocketAddr,
    _task: AbortTask,
    state: Arc<HandlerState>,
    observations: mpsc::UnboundedReceiver<HandlerObservation>,
}

impl WireServer {
    async fn observe(&mut self, expected: HandlerObservation) {
        assert_eq!(self.observations.recv().await, Some(expected));
    }
}

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
    state: Arc<HandlerState>,
) -> (i32, String) {
    state.calls.fetch_add(1, Ordering::Release);
    state
        .observations
        .send(HandlerObservation::Entered)
        .unwrap();
    let mut count = 0;
    let mut retried = false;
    let outcome = loop {
        match stream.recv().await {
            Ok(Some(event)) => {
                assert_eq!(event.as_event().unwrap().data(), Some("hello"));
                count += 1;
                state
                    .observations
                    .send(HandlerObservation::Event(count))
                    .unwrap();
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
                state
                    .observations
                    .send(HandlerObservation::Timeout(outcome, error.timeout()))
                    .unwrap();
                if retry && !retried {
                    retried = true;
                    // The test queues the remaining bytes before allowing another
                    // recv, avoiding a race against a second real-time deadline.
                    state.resume_retry.notified().await;
                    continue;
                }
                break outcome;
            }
        }
    };
    let result = (count, outcome.to_owned());
    state.results.lock().unwrap().push(result.clone());
    if state.hold_completion {
        state.finish.notified().await;
    }
    result
}

async fn health(_input: HealthInput) -> HealthOutput {
    HealthOutput {}
}

async fn start_server(hold_completion: bool) -> WireServer {
    let (observations, receiver) = mpsc::unbounded_channel();
    let state = Arc::new(HandlerState {
        calls: AtomicUsize::new(0),
        results: Mutex::new(Vec::new()),
        observations,
        resume_retry: Notify::new(),
        finish: Notify::new(),
        hold_completion,
    });
    let config = crate::WireServiceConfig::builder().build();
    let app = crate::WireService::builder(config)
        .publish_events({
            let state = state.clone();
            move |input: PublishEventsInput| {
                let state = state.clone();
                async move {
                    let (count, outcome) = events(input.events, false, state).await;
                    Ok::<_, crate::error::PublishEventsError>(PublishEventsOutput {
                        count,
                        outcome,
                    })
                }
            }
        })
        .retry_events({
            let state = state.clone();
            move |input: RetryEventsInput| {
                let state = state.clone();
                async move {
                    let (count, outcome) = events(input.events, true, state).await;
                    Ok::<_, crate::error::RetryEventsError>(RetryEventsOutput { count, outcome })
                }
            }
        })
        .no_idle_events({
            let state = state.clone();
            move |input: NoIdleEventsInput| {
                let state = state.clone();
                async move {
                    let (count, outcome) = events(input.events, false, state).await;
                    Ok::<_, crate::error::NoIdleEventsError>(NoIdleEventsOutput { count, outcome })
                }
            }
        })
        .global_idle_events({
            let state = state.clone();
            move |input: GlobalIdleEventsInput| {
                let state = state.clone();
                async move {
                    let (count, outcome) = events(input.events, false, state).await;
                    Ok::<_, crate::error::GlobalIdleEventsError>(GlobalIdleEventsOutput {
                        count,
                        outcome,
                    })
                }
            }
        })
        .no_initial_events({
            let state = state.clone();
            move |input: NoInitialEventsInput| {
                let state = state.clone();
                async move {
                    let (count, outcome) = events(input.events, false, state).await;
                    Ok::<_, crate::error::NoInitialEventsError>(NoInitialEventsOutput {
                        count,
                        outcome,
                    })
                }
            }
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
    WireServer {
        addr,
        _task: task,
        state,
        observations: receiver,
    }
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

    fn response_outcome(self, body: &[u8]) -> (i32, String) {
        if self != Self::Cbor {
            let value: serde_json::Value = serde_json::from_slice(body).unwrap();
            return (
                value["count"].as_i64().unwrap().try_into().unwrap(),
                value["outcome"].as_str().unwrap().to_owned(),
            );
        }
        let mut decoder = aws_smithy_cbor::Decoder::new(body);
        let length = decoder.map().unwrap();
        let mut count = None;
        let mut outcome = None;
        let mut fields = 0;
        loop {
            if let Some(length) = length {
                if fields == length {
                    break;
                }
            } else if decoder.datatype().unwrap() == aws_smithy_cbor::data::Type::Break {
                decoder.skip().unwrap();
                break;
            }
            match decoder.str().unwrap().as_ref() {
                "count" => {
                    assert!(count.is_none(), "duplicate count");
                    count = Some(decoder.integer().unwrap());
                }
                "outcome" => {
                    assert!(outcome.is_none(), "duplicate outcome");
                    outcome = Some(decoder.str().unwrap().into_owned());
                }
                field => panic!("unexpected response field: {field}"),
            }
            fields += 1;
        }
        assert_eq!(decoder.position(), body.len(), "trailing response bytes");
        (count.unwrap(), outcome.unwrap())
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
    response: JoinHandle<(http_1x::StatusCode, Bytes)>,
    connection: AbortTask<Result<(), hyper::Error>>,
    protocol: WireProtocol,
    state: Arc<HandlerState>,
}

impl Drop for WireClient {
    fn drop(&mut self) {
        self.response.abort();
    }
}

impl WireClient {
    async fn connect(
        server: &WireServer,
        protocol: WireProtocol,
        operation: &str,
        streaming: bool,
    ) -> Self {
        let stream = tokio::net::TcpStream::connect(server.addr).await.unwrap();
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
            let body = response.into_body().collect().await.unwrap().to_bytes();
            (status, body)
        });
        let client = Self {
            body: Some(body),
            response,
            connection,
            protocol,
            state: server.state.clone(),
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
        self.state.finish.notify_one();
        let (status, body) = (&mut self.response).await.unwrap();
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
                self.state.results.lock().unwrap().is_empty(),
                "rejection invoked the handler"
            );
            assert_eq!(self.state.calls.load(Ordering::Acquire), 0);
        } else {
            assert!(status.is_success(), "{status}");
            if let Some((outcome, count)) = outcome {
                let results = std::mem::take(&mut *self.state.results.lock().unwrap());
                assert_eq!(results, vec![(count, outcome.to_owned())]);
                assert_eq!(self.state.calls.load(Ordering::Acquire), 1);
                assert_eq!(
                    self.protocol.response_outcome(&body),
                    (count, outcome.to_owned()),
                    "serialized response must match the handler outcome",
                );
            }
        }
    }

    async fn pending_for(&mut self, duration: Duration) {
        assert!(
            timeout(duration, &mut self.response).await.is_err(),
            "request ended too early"
        );
    }
}

// Precise deadline boundaries and slow-drip schedules are covered by the
// start_paused tests in event_stream_message_timeout.rs. TCP tests verify
// propagation, serialization, and connection cleanup with real socket I/O.
async fn wire_scenario(protocol: WireProtocol, name: &str) {
    let mut server = start_server(
        name == "idle_expiry_releases_connection_slot_and_server_still_serves_requests",
    )
    .await;
    match name {
        "initial_no_bytes_rejected_before_handler" | "initial_partial_rejected_before_handler" => {
            let client = WireClient::connect(&server, protocol, "PublishEvents", true).await;
            if name == "initial_partial_rejected_before_handler" {
                client.send(protocol.frame(true).slice(..2)).await;
            }
            client.expect(None, true).await;
        }
        "stream_only_RPC_missing_first_frame_rejected_before_handler"
        | "stream_only_RPC_partial_first_frame_uses_initial_deadline" => {
            let client = WireClient::connect(&server, protocol, "NoInitialEvents", true).await;
            if name == "stream_only_RPC_partial_first_frame_uses_initial_deadline" {
                client.send(protocol.frame(false).slice(..2)).await;
            }
            client.expect(None, true).await;
        }
        "stream_only_RPC_first_event_is_buffered_before_handler" => {
            let mut client = WireClient::connect(&server, protocol, "NoInitialEvents", true).await;
            assert_eq!(server.state.calls.load(Ordering::Acquire), 0);
            client.send(protocol.frame(false)).await;
            client.finish();
            server.observe(HandlerObservation::Entered).await;
            server.observe(HandlerObservation::Event(1)).await;
            client.expect(Some(("eof", 1)), false).await;
        }
        "first_event_idle_error_reaches_handler" => {
            let client = WireClient::connect(&server, protocol, "PublishEvents", true).await;
            client.initial(protocol).await;
            server.observe(HandlerObservation::Entered).await;
            server
                .observe(HandlerObservation::Timeout("idle", Duration::from_secs(1)))
                .await;
            client.expect(Some(("idle", 0)), false).await;
        }
        "idle_after_valid_event_reaches_handler_and_closes_TCP" => {
            let client = WireClient::connect(&server, protocol, "PublishEvents", true).await;
            client.initial(protocol).await;
            server.observe(HandlerObservation::Entered).await;
            client.send(protocol.frame(false)).await;
            server.observe(HandlerObservation::Event(1)).await;
            server
                .observe(HandlerObservation::Timeout("idle", Duration::from_secs(1)))
                .await;
            client.expect(Some(("idle", 1)), false).await;
        }
        "partial_length_prefix_completion_error_reaches_handler" => {
            let client = WireClient::connect(&server, protocol, "PublishEvents", true).await;
            client.initial(protocol).await;
            server.observe(HandlerObservation::Entered).await;
            // Stop inside the four-byte length prefix. Exact slow-drip timing
            // is exercised with paused time rather than client-side sleeps.
            client.send(protocol.frame(false).slice(..3)).await;
            server
                .observe(HandlerObservation::Timeout(
                    "completion",
                    Duration::from_secs(1),
                ))
                .await;
            client.expect(Some(("completion", 0)), false).await;
        }
        "complete_events_are_delivered_in_order" => {
            let mut client = WireClient::connect(&server, protocol, "PublishEvents", true).await;
            client.initial(protocol).await;
            server.observe(HandlerObservation::Entered).await;
            for count in 1..=3 {
                client.send(protocol.frame(false)).await;
                server.observe(HandlerObservation::Event(count)).await;
            }
            client.finish();
            client.expect(Some(("eof", 3)), false).await;
        }
        "NoIdleEvents_remains_open_beyond_short_idle_override"
        | "GlobalIdleEvents_remains_open_beyond_short_idle_override" => {
            let operation = name.split('_').next().unwrap();
            let mut client = WireClient::connect(&server, protocol, operation, true).await;
            client.initial(protocol).await;
            server.observe(HandlerObservation::Entered).await;
            // The short override is 1s; the inherited deadline is 5s. Leave
            // 3.5s of scheduling margin for the eventual frame and EOF.
            client.pending_for(Duration::from_millis(1500)).await;
            client.send(protocol.frame(false)).await;
            client.finish();
            client.expect(Some(("eof", 1)), false).await;
        }
        "inherited_global_idle_deadline_reaches_handler" => {
            let client = WireClient::connect(&server, protocol, "GlobalIdleEvents", true).await;
            client.initial(protocol).await;
            server.observe(HandlerObservation::Entered).await;
            server
                .observe(HandlerObservation::Timeout("idle", Duration::from_secs(5)))
                .await;
            client.expect(Some(("idle", 0)), false).await;
        }
        "handler_can_retry_idle_and_receive_event"
        | "handler_can_retry_completion_and_receive_event" => {
            let partial = name == "handler_can_retry_completion_and_receive_event";
            let mut client = WireClient::connect(&server, protocol, "RetryEvents", true).await;
            client.initial(protocol).await;
            server.observe(HandlerObservation::Entered).await;
            let frame = protocol.frame(false);
            if partial {
                client.send(frame.slice(..2)).await;
            }
            server
                .observe(HandlerObservation::Timeout(
                    if partial { "completion" } else { "idle" },
                    Duration::from_secs(1),
                ))
                .await;
            // The handler has observed the actual timeout and is waiting for
            // this signal. Queue the rest before giving recv a fresh window.
            client
                .send(if partial { frame.slice(2..) } else { frame })
                .await;
            client.finish();
            server.state.resume_retry.notify_one();
            server.observe(HandlerObservation::Event(1)).await;
            client.expect(Some(("eof-after-timeout", 1)), false).await;
        }
        "idle_expiry_releases_connection_slot_and_server_still_serves_requests" => {
            let first = WireClient::connect(&server, protocol, "PublishEvents", true).await;
            first.initial(protocol).await;
            server.observe(HandlerObservation::Entered).await;
            server
                .observe(HandlerObservation::Timeout("idle", Duration::from_secs(1)))
                .await;
            // This fixture holds the first handler after expiry until expect
            // releases it, so it cannot finish while we check the queued request.
            let mut second = WireClient::connect(&server, protocol, "Health", false).await;
            second.finish();
            second.pending_for(Duration::from_millis(100)).await;
            first.expect(Some(("idle", 0)), false).await;
            second.expect(None, false).await;
        }
        _ => panic!("unknown wire scenario: {name}"),
    }
}

async fn wire_scenarios(protocol: WireProtocol) {
    let start = Instant::now();
    let mut scenarios = Vec::new();
    if protocol == WireProtocol::Cbor {
        scenarios.extend([
            "initial_no_bytes_rejected_before_handler",
            "initial_partial_rejected_before_handler",
        ]);
    }
    if protocol != WireProtocol::Rest {
        scenarios.extend([
            "stream_only_RPC_missing_first_frame_rejected_before_handler",
            "stream_only_RPC_partial_first_frame_uses_initial_deadline",
            "stream_only_RPC_first_event_is_buffered_before_handler",
        ]);
    }
    scenarios.extend([
        "first_event_idle_error_reaches_handler",
        "idle_after_valid_event_reaches_handler_and_closes_TCP",
        "partial_length_prefix_completion_error_reaches_handler",
        "complete_events_are_delivered_in_order",
        "NoIdleEvents_remains_open_beyond_short_idle_override",
        "GlobalIdleEvents_remains_open_beyond_short_idle_override",
        "inherited_global_idle_deadline_reaches_handler",
        "handler_can_retry_idle_and_receive_event",
        "handler_can_retry_completion_and_receive_event",
        "idle_expiry_releases_connection_slot_and_server_still_serves_requests",
    ]);
    for name in &scenarios {
        timeout(Duration::from_secs(15), wire_scenario(protocol, name))
            .await
            .unwrap_or_else(|_| {
                panic!("{} {name} exceeded its 15-second watchdog", protocol.name())
            });
        println!("PASS {} {name}", protocol.name());
    }
    assert_eq!(
        scenarios.len(),
        match protocol {
            WireProtocol::Rest => 10,
            WireProtocol::Json => 13,
            WireProtocol::Cbor => 15,
        }
    );
    println!(
        "PASS {} all {} TCP scenarios in {:?}",
        protocol.name(),
        scenarios.len(),
        start.elapsed()
    );
}
