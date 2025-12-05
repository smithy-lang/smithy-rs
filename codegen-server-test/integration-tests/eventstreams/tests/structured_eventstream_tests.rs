/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http::event_stream::EventStreamSender;
use aws_smithy_runtime::test_util::capture_test_logs::show_filtered_test_logs;
use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
use bytes::Bytes;
use eventstreams::{ManualEventStreamClient, RecvError};
use rpcv2cbor_extras::model::{Event, Events};
use rpcv2cbor_extras::sigv4_event_stream::SignedEvent;
use rpcv2cbor_extras::{error, input, output, RpcV2CborService, RpcV2CborServiceConfig};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

#[derive(Debug, Default, Clone)]
struct StreamingOperationState {
    events: Vec<SignedEvent<Events>>,
    num_calls: usize,
    initial_signature: Option<Vec<u8>>,
}

#[derive(Debug, Default, Clone)]
struct StreamingOperationWithInitialDataState {
    initial_data: Option<String>,
    events: Vec<SignedEvent<Events>>,
    #[allow(dead_code)]
    num_calls: usize,
    initial_signature: Option<Vec<u8>>,
}

#[derive(Debug, Default, Clone)]
struct StreamingOperationWithOptionalDataState {
    optional_data: Option<String>,
    events: Vec<SignedEvent<Events>>,
    #[allow(dead_code)]
    num_calls: usize,
    initial_signature: Option<Vec<u8>>,
}

#[derive(Debug, Default, Clone)]
struct ServerState {
    streaming_operation: StreamingOperationState,
    streaming_operation_with_initial_data: StreamingOperationWithInitialDataState,
    streaming_operation_with_optional_data: StreamingOperationWithOptionalDataState,
}

struct TestServer {
    addr: std::net::SocketAddr,
    state: Arc<Mutex<ServerState>>,
}

impl TestServer {
    async fn start() -> Self {
        let state = Arc::new(Mutex::new(ServerState::default()));
        let handler_state = state.clone();
        let handler_state2 = state.clone();
        let handler_state3 = state.clone();
        let handler_state4 = state.clone();

        let config = RpcV2CborServiceConfig::builder().build();
        let app = RpcV2CborService::builder::<hyper0::Body, _, _, _>(config)
            .streaming_operation(move |input| {
                let state = handler_state.clone();
                streaming_operation_handler(input, state)
            })
            .streaming_operation_with_initial_data(move |input| {
                let state = handler_state2.clone();
                streaming_operation_with_initial_data_handler(input, state)
            })
            .streaming_operation_with_initial_response(move |input| {
                let state = handler_state3.clone();
                streaming_operation_with_initial_response_handler(input, state)
            })
            .streaming_operation_with_optional_data(move |input| {
                let state = handler_state4.clone();
                streaming_operation_with_optional_data_handler(input, state)
            })
            .build_unchecked();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let make_service = app.into_make_service();
            let server = hyper0::Server::from_tcp(listener.into_std().unwrap())
                .unwrap()
                .http2_only(true)
                .serve(make_service);
            server.await.unwrap();
        });

        Self { addr, state }
    }

    fn streaming_operation_events(&self) -> Vec<SignedEvent<Events>> {
        self.state
            .lock()
            .unwrap()
            .streaming_operation
            .events
            .clone()
    }

    fn streaming_operation_with_initial_data_events(&self) -> Vec<SignedEvent<Events>> {
        self.state
            .lock()
            .unwrap()
            .streaming_operation_with_initial_data
            .events
            .clone()
    }

    fn initial_data(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap()
            .streaming_operation_with_initial_data
            .initial_data
            .clone()
    }

    fn streaming_operation_with_optional_data_events(&self) -> Vec<SignedEvent<Events>> {
        self.state
            .lock()
            .unwrap()
            .streaming_operation_with_optional_data
            .events
            .clone()
    }

    fn optional_data(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap()
            .streaming_operation_with_optional_data
            .optional_data
            .clone()
    }

    fn initial_signature(&self) -> Option<Vec<u8>> {
        self.state
            .lock()
            .unwrap()
            .streaming_operation_with_initial_data
            .initial_signature
            .clone()
    }

    fn streaming_operation_initial_signature(&self) -> Option<Vec<u8>> {
        self.state
            .lock()
            .unwrap()
            .streaming_operation
            .initial_signature
            .clone()
    }
}

async fn streaming_operation_handler(
    mut input: input::StreamingOperationInput,
    state: Arc<Mutex<ServerState>>,
) -> Result<output::StreamingOperationOutput, error::StreamingOperationError> {
    state.lock().unwrap().streaming_operation.num_calls += 1;
    state.lock().unwrap().streaming_operation.initial_signature = input
        .events
        .initial_signature()
        .map(|s| s.chunk_signature.to_vec());

    let state_clone = state.clone();
    tokio::spawn(async move {
        while let Ok(Some(signed_event)) = input.events.recv().await {
            tracing::debug!(
                "streaming_operation received event: {:?}",
                signed_event.message
            );
            state_clone
                .lock()
                .unwrap()
                .streaming_operation
                .events
                .push(signed_event);
        }
    });

    Ok(output::StreamingOperationOutput::builder()
        .events(EventStreamSender::once(Ok(Events::A(Event {}))))
        .build()
        .unwrap())
}

async fn streaming_operation_with_initial_data_handler(
    mut input: input::StreamingOperationWithInitialDataInput,
    state: Arc<Mutex<ServerState>>,
) -> Result<
    output::StreamingOperationWithInitialDataOutput,
    error::StreamingOperationWithInitialDataError,
> {
    state.lock().unwrap().streaming_operation.num_calls += 1;
    state
        .lock()
        .unwrap()
        .streaming_operation_with_initial_data
        .initial_data = Some(input.initial_data);
    state
        .lock()
        .unwrap()
        .streaming_operation_with_initial_data
        .initial_signature = input
        .events
        .initial_signature()
        .map(|s| s.chunk_signature.to_vec());

    let state_clone = state.clone();
    tokio::spawn(async move {
        while let Ok(Some(signed_event)) = input.events.recv().await {
            tracing::debug!(
                "streaming_operation_with_initial_data received event: {:?}",
                signed_event.message
            );
            state_clone
                .lock()
                .unwrap()
                .streaming_operation_with_initial_data
                .events
                .push(signed_event);
        }
    });

    Ok(output::StreamingOperationWithInitialDataOutput::builder()
        .events(EventStreamSender::once(Ok(Events::A(Event {}))))
        .build()
        .unwrap())
}

async fn streaming_operation_with_initial_response_handler(
    mut input: input::StreamingOperationWithInitialResponseInput,
    _state: Arc<Mutex<ServerState>>,
) -> Result<
    output::StreamingOperationWithInitialResponseOutput,
    error::StreamingOperationWithInitialResponseError,
> {
    tokio::spawn(async move {
        while let Ok(Some(event)) = input.events.recv().await {
            tracing::debug!(
                "streaming_operation_with_initial_response received event: {:?}",
                event
            );
        }
    });

    Ok(
        output::StreamingOperationWithInitialResponseOutput::builder()
            .response_data("test response data".to_string())
            .events(EventStreamSender::once(Ok(Events::A(Event {}))))
            .build()
            .unwrap(),
    )
}

async fn streaming_operation_with_optional_data_handler(
    mut input: input::StreamingOperationWithOptionalDataInput,
    state: Arc<Mutex<ServerState>>,
) -> Result<
    output::StreamingOperationWithOptionalDataOutput,
    error::StreamingOperationWithOptionalDataError,
> {
    state.lock().unwrap().streaming_operation.num_calls += 1;
    state
        .lock()
        .unwrap()
        .streaming_operation_with_optional_data
        .optional_data = input.optional_data;
    state
        .lock()
        .unwrap()
        .streaming_operation_with_optional_data
        .initial_signature = input
        .events
        .initial_signature()
        .map(|s| s.chunk_signature.to_vec());

    let state_clone = state.clone();
    tokio::spawn(async move {
        while let Ok(Some(event)) = input.events.recv().await {
            tracing::debug!(
                "streaming_operation_with_optional_data received event: {:?}",
                event
            );
            state_clone
                .lock()
                .unwrap()
                .streaming_operation_with_optional_data
                .events
                .push(event);
        }
    });

    Ok(output::StreamingOperationWithOptionalDataOutput::builder()
        .optional_response_data(Some("optional response".to_string()))
        .events(EventStreamSender::once(Ok(Events::A(Event {}))))
        .build()
        .unwrap())
}

/// TestHarness that launches a server and attaches a client
///
/// It allows sending event stream messages and reading the results.
struct TestHarness {
    server: TestServer,
    client: ManualEventStreamClient,
    initial_response: Option<Message>,
}

impl TestHarness {
    async fn new(operation: &str) -> Self {
        let server = TestServer::start().await;
        let path = format!("/service/RpcV2CborService/operation/{operation}");
        let client = ManualEventStreamClient::connect_to_service(
            server.addr,
            &path,
            vec![("Smithy-Protocol", "rpc-v2-cbor")],
        )
        .await
        .unwrap();

        Self {
            server,
            client,
            initial_response: None,
        }
    }

    async fn send_initial_data(&mut self, data: &str) {
        let msg = build_initial_data_message(data);
        self.client.send(msg).await.ok();
    }

    async fn send_event(&mut self, event_type: &str) {
        let msg = build_event(event_type);
        self.client.send(msg).await.unwrap();
    }

    async fn expect_message(&mut self) -> Message {
        let msg = self.client.recv().await.unwrap().unwrap();

        // If this is an initial-response, store it and get the next message
        if get_event_type(&msg) == "initial-response" {
            self.initial_response = Some(msg);
            self.client.recv().await.unwrap().unwrap()
        } else {
            msg
        }
    }

    async fn recv(&mut self) -> Option<Result<Message, RecvError>> {
        self.client.recv().await
    }
}

fn build_initial_request() -> Message {
    let headers = vec![
        Header::new(":message-type", HeaderValue::String("event".into())),
        Header::new(":event-type", HeaderValue::String("initial-request".into())),
        Header::new(
            ":content-type",
            HeaderValue::String("application/cbor".into()),
        ),
    ];
    let empty_cbor = Bytes::from(vec![0xbf, 0xff]);
    Message::new_from_parts(headers, empty_cbor)
}

fn build_initial_data_message(data: &str) -> Message {
    let headers = vec![
        Header::new(":message-type", HeaderValue::String("event".into())),
        Header::new(":event-type", HeaderValue::String("initial-request".into())),
        Header::new(
            ":content-type",
            HeaderValue::String("application/cbor".into()),
        ),
    ];

    // Serialize using CBOR: { "initialData": data }
    let mut encoder = aws_smithy_cbor::Encoder::new(Vec::new());
    encoder.begin_map();
    encoder.str("initialData").str(data);
    encoder.end();
    let payload = Bytes::from(encoder.into_writer());

    Message::new_from_parts(headers, payload)
}

fn build_event(event_type: &str) -> Message {
    let headers = vec![
        Header::new(":message-type", HeaderValue::String("event".into())),
        Header::new(
            ":event-type",
            HeaderValue::String(event_type.to_string().into()),
        ),
        Header::new(
            ":content-type",
            HeaderValue::String("application/cbor".into()),
        ),
    ];
    let empty_cbor = Bytes::from(vec![0xbf, 0xff]);
    Message::new_from_parts(headers, empty_cbor)
}

fn sign_message(inner_message: Message, signature: &[u8], timestamp_secs: i64) -> Message {
    use aws_smithy_eventstream::frame::write_message_to;

    let mut inner_bytes = Vec::new();
    write_message_to(&inner_message, &mut inner_bytes).unwrap();

    let headers = vec![
        Header::new(
            ":chunk-signature",
            HeaderValue::ByteArray(Bytes::from(signature.to_vec())),
        ),
        Header::new(
            ":date",
            HeaderValue::Timestamp(aws_smithy_types::DateTime::from_secs(timestamp_secs)),
        ),
    ];

    Message::new_from_parts(headers, Bytes::from(inner_bytes))
}

fn build_sigv4_signed_event_with_signature(event_type: &str, signature: &[u8]) -> Message {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    sign_message(build_event(event_type), signature, timestamp as i64)
}

fn build_sigv4_signed_initial_data(data: &str, signature: &[u8], timestamp_secs: i64) -> Message {
    sign_message(build_initial_data_message(data), signature, timestamp_secs)
}

fn get_event_type(msg: &Message) -> &str {
    msg.headers()
        .iter()
        .find(|h| h.name().as_str() == ":event-type")
        .unwrap()
        .value()
        .as_string()
        .unwrap()
        .as_str()
}

#[tokio::test]
async fn test_streaming_operation_with_initial_data() {
    let mut harness = TestHarness::new("StreamingOperationWithInitialData").await;
    harness.send_initial_data("test-data").await;

    harness.send_event("A").await;

    let resp = harness.expect_message().await;
    assert_eq!(get_event_type(&resp), "A");
    assert_eq!(
        harness
            .server
            .streaming_operation_with_initial_data_events()
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>(),
        vec![Events::A(Event {})]
    );
    // verify that we parsed the initial data properly
    assert_eq!(harness.server.initial_data(), Some("test-data".to_string()));
}

/// Test that operations without modeled initial messages don't hang.
/// This verifies the fix for issue #4435 where recv() would block waiting
/// for an initial message that was never modeled.
#[tokio::test]
async fn test_no_hang_without_initial_message() {
    let mut harness = TestHarness::new("StreamingOperation").await;

    // Wait for the response to be ready without sending any messages
    // This should not hang because StreamingOperation has no modeled initial-request
    harness.client.wait_for_response_ready().await;

    // Now send an event and verify we can receive it
    harness.send_event("A").await;
    let resp = harness.expect_message().await;
    assert_eq!(get_event_type(&resp), "A");
}

/// Test that operations WITH modeled initial messages DO wait for them.
/// This verifies that try_recv_initial() blocks and the response doesn't become ready
/// until the initial-request is sent.
#[tokio::test]
async fn test_waits_for_initial_message_when_modeled() {
    let mut harness = TestHarness::new("StreamingOperationWithInitialData").await;

    // Try to wait for response with a timeout - it should timeout because we haven't sent initial data
    let result = tokio::time::timeout(
        tokio::time::Duration::from_millis(100),
        harness.client.wait_for_response_ready(),
    )
    .await;
    assert!(
        result.is_err(),
        "Response should not be ready without initial data"
    );

    // Now send the initial data
    harness.send_initial_data("test-data").await;

    // Now the response should become ready
    harness.client.wait_for_response_ready().await;

    // Send an event and verify it works
    harness.send_event("A").await;
    let resp = harness.expect_message().await;
    assert_eq!(get_event_type(&resp), "A");
    assert_eq!(harness.server.initial_data(), Some("test-data".to_string()));
}

/// StreamingOperationWithInitialData has a mandatory initial data field.
/// If we don't send this field, we'll never hit the handler.
#[tokio::test]
async fn test_streaming_operation_with_initial_data_missing() {
    let _logs = show_filtered_test_logs(
        "aws_smithy_http_server=trace,hyper_util=debug,rpcv2cbor_extras=trace",
    );
    let mut harness = TestHarness::new("StreamingOperationWithInitialData").await;

    harness.send_event("A").await;

    assert!(harness.recv().await.is_none());

    // assert the server never received the request
    assert_eq!(
        harness
            .server
            .streaming_operation_with_initial_data_events()
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>(),
        vec![]
    );
}

/// Test that when alwaysSendEventStreamInitialResponse is disabled, no initial-response is sent
#[tokio::test]
async fn test_server_no_initial_response_when_disabled() {
    use rpcv2cbor_extras_no_initial_response::output;
    use rpcv2cbor_extras_no_initial_response::{RpcV2CborService, RpcV2CborServiceConfig};

    let config = RpcV2CborServiceConfig::builder().build();
    let app = RpcV2CborService::builder::<hyper0::Body, _, _, _>(config)
        .streaming_operation_with_initial_data(move |mut input: rpcv2cbor_extras_no_initial_response::input::StreamingOperationWithInitialDataInput| async move {
            let _ev = input.events.recv().await;
            Ok(output::StreamingOperationWithInitialDataOutput::builder()
                .events(aws_smithy_http::event_stream::EventStreamSender::once(Ok(rpcv2cbor_extras_no_initial_response::model::Events::A(rpcv2cbor_extras_no_initial_response::model::Event {}))))
                .build()
                .unwrap())
        })
        .build_unchecked();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let make_service = app.into_make_service();
        let server = hyper0::Server::from_tcp(listener.into_std().unwrap())
            .unwrap()
            .http2_only(true)
            .serve(make_service);
        server.await.unwrap();
    });

    let path = "/service/RpcV2CborService/operation/StreamingOperationWithInitialData";
    let mut client = ManualEventStreamClient::connect_to_service(
        addr,
        path,
        vec![("Smithy-Protocol", "rpc-v2-cbor")],
    )
    .await
    .unwrap();

    // Send initial data and event
    let msg = build_initial_data_message("test-data");
    client.send(msg).await.ok();
    let msg = build_event("A");
    client.send(msg).await.unwrap();

    // Should receive event directly (no initial-response)
    let resp = client.recv().await.unwrap().unwrap();
    assert_eq!(
        get_event_type(&resp),
        "A",
        "Should receive event directly without initial-response"
    );
}

/// Test that server sends initial-response for RPC protocols
#[tokio::test]
async fn test_server_sends_initial_response() {
    let mut harness = TestHarness::new("StreamingOperationWithInitialData").await;
    harness.send_initial_data("test-data").await;
    harness.send_event("A").await;

    // Server should send initial-response before any events
    let initial_response = harness.client.try_recv_initial_response().await;
    assert!(
        initial_response.is_some(),
        "Server should send initial-response message"
    );

    let initial_msg = initial_response.unwrap().unwrap();
    assert_eq!(get_event_type(&initial_msg), "initial-response");

    // Then we should get the actual event
    let resp = harness.expect_message().await;
    assert_eq!(get_event_type(&resp), "A");
}

/// Test that server sends initial-response with actual data for operations that have non-eventstream output members
#[tokio::test]
async fn test_server_sends_initial_response_with_data() {
    let mut harness = TestHarness::new("StreamingOperationWithInitialResponse").await;
    harness.send_event("A").await;

    let resp = harness.expect_message().await;
    assert_eq!(get_event_type(&resp), "A");

    // Check that initial-response was received with actual data
    assert!(harness.initial_response.is_some());
    let initial_resp = harness.initial_response.as_ref().unwrap();
    assert_eq!(get_event_type(initial_resp), "initial-response");

    // The initial response should contain serialized responseData
    assert!(!initial_resp.payload().is_empty());

    // Parse the CBOR payload to verify the responseData field
    let decoder = &mut aws_smithy_cbor::Decoder::new(initial_resp.payload());
    decoder.map().unwrap(); // Start reading the map

    // Read the key-value pair
    let key = decoder.str().unwrap();
    assert_eq!(key, "responseData");
    let value = decoder.string().unwrap();
    assert_eq!(value, "test response data");
}

/// Test streaming operation with optional data - verifies Smithy spec requirement:
/// "Clients and servers MUST NOT fail if an initial-request or initial-response
/// is not received for an initial message that contains only optional members."
#[tokio::test]
async fn test_streaming_operation_with_optional_data() {
    let mut harness = TestHarness::new("StreamingOperationWithOptionalData").await;

    // Send event without providing optional data - should work
    harness.send_event("A").await;

    let resp = harness.expect_message().await;
    assert_eq!(get_event_type(&resp), "A");
    assert_eq!(
        harness
            .server
            .streaming_operation_with_optional_data_events()
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>(),
        vec![Events::A(Event {})]
    );
    // Verify optional data was not provided
    assert_eq!(harness.server.optional_data(), None);
}

/// Test that SigV4-framed initial-request messages are properly handled.
/// This verifies the fix for issue #4397 where try_recv_initial_request
/// can now see inside the SigV4 envelope to detect the initial-request event type.
#[tokio::test]
async fn test_sigv4_framed_initial_request_with_data() {
    let _logs = show_filtered_test_logs(
        "aws_smithy_http_server=trace,hyper_util=debug,rpcv2cbor_extras=trace",
    );
    let mut harness = TestHarness::new("StreamingOperationWithInitialData").await;

    // Send a SigV4-framed initial-request with data
    let signed_initial_request = build_sigv4_signed_initial_data(
        "test-data",
        b"example298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        1700000000,
    );
    harness.client.send(signed_initial_request).await.unwrap();

    harness.send_event("A").await;

    // The server should now properly extract the initial-request from the SigV4 envelope
    let resp = harness.expect_message().await;
    assert_eq!(get_event_type(&resp), "A");

    // Verify the server received and parsed the initial data from inside the SigV4 envelope
    assert_eq!(harness.server.initial_data(), Some("test-data".to_string()));
    assert_eq!(
        harness.server.initial_signature(),
        Some(b"example298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_vec())
    );
}

#[derive(Debug, Clone, Copy)]
enum InitialMessage {
    None,
    Unsigned,
    Signed,
}

#[derive(Debug, Clone)]
struct EventStreamTestCase {
    initial: InitialMessage,
    events_signed: Vec<bool>,
}

/// Comprehensive test matrix for SigV4 event stream combinations
#[rstest::rstest]
#[case::no_initial_unsigned_events(EventStreamTestCase { initial: InitialMessage::None, events_signed: vec![false, false] })]
#[case::no_initial_signed_events(EventStreamTestCase { initial: InitialMessage::None, events_signed: vec![true, true] })]
#[case::no_initial_mixed_events(EventStreamTestCase { initial: InitialMessage::None, events_signed: vec![false, true] })]
#[case::unsigned_initial_unsigned_events(EventStreamTestCase { initial: InitialMessage::Unsigned, events_signed: vec![false, false] })]
#[case::unsigned_initial_signed_events(EventStreamTestCase { initial: InitialMessage::Unsigned, events_signed: vec![true, true] })]
#[case::unsigned_initial_mixed_events(EventStreamTestCase { initial: InitialMessage::Unsigned, events_signed: vec![false, true] })]
#[case::signed_initial_unsigned_events(EventStreamTestCase { initial: InitialMessage::Signed, events_signed: vec![false, false] })]
#[case::signed_initial_signed_events(EventStreamTestCase { initial: InitialMessage::Signed, events_signed: vec![true, true] })]
#[case::signed_initial_mixed_events(EventStreamTestCase { initial: InitialMessage::Signed, events_signed: vec![false, true] })]
#[case::no_events(EventStreamTestCase { initial: InitialMessage::None, events_signed: vec![] })]
#[case::many_signed_events(EventStreamTestCase { initial: InitialMessage::Signed, events_signed: vec![true; 100] })]
#[case::many_unsigned_events(EventStreamTestCase { initial: InitialMessage::None, events_signed: vec![false; 100] })]
#[tokio::test]
async fn test_sigv4_event_stream_matrix(#[case] test_case: EventStreamTestCase) {
    let mut harness = TestHarness::new("StreamingOperation").await;

    // Send initial message if specified
    match test_case.initial {
        InitialMessage::None => {}
        InitialMessage::Unsigned => {
            harness.client.send(build_initial_request()).await.unwrap();
        }
        InitialMessage::Signed => {
            let signed_initial = sign_message(build_initial_request(), b"initial-sig", 1700000000);
            harness.client.send(signed_initial).await.unwrap();
        }
    }

    // Send events
    for (i, &signed) in test_case.events_signed.iter().enumerate() {
        let event_type = if i % 2 == 0 { "A" } else { "B" };
        if signed {
            let sig = format!("sig-event-{i}");
            let signed_event = build_sigv4_signed_event_with_signature(event_type, sig.as_bytes());
            harness.client.send(signed_event).await.unwrap();
        } else {
            harness.send_event(event_type).await;
        }
    }

    // Receive response (only if we sent events)
    if !test_case.events_signed.is_empty() {
        let resp = harness.expect_message().await;
        assert_eq!(get_event_type(&resp), "A");
    }

    // Verify events
    let events = harness.server.streaming_operation_events();
    assert_eq!(events.len(), test_case.events_signed.len());

    for (i, &signed) in test_case.events_signed.iter().enumerate() {
        let expected_event = if i % 2 == 0 {
            Events::A(Event {})
        } else {
            Events::B(Event {})
        };
        assert_eq!(events[i].message, expected_event);

        if signed {
            assert!(
                events[i].signature.is_some(),
                "Event {i} should have signature"
            );
            let expected_sig = format!("sig-event-{i}");
            assert_eq!(
                events[i].signature.as_ref().unwrap().chunk_signature,
                expected_sig.as_bytes()
            );
        } else {
            assert!(
                events[i].signature.is_none(),
                "Event {i} should not have signature"
            );
        }
    }

    // Verify initial signature
    match test_case.initial {
        InitialMessage::Signed => {
            assert_eq!(
                harness.server.streaming_operation_initial_signature(),
                Some(b"initial-sig".to_vec())
            );
        }
        InitialMessage::None | InitialMessage::Unsigned => {
            assert_eq!(harness.server.streaming_operation_initial_signature(), None);
        }
    }
}

/// Test signed initial data with signed events
#[tokio::test]
async fn test_sigv4_signed_initial_data_with_signed_events() {
    let mut harness = TestHarness::new("StreamingOperationWithInitialData").await;

    // Send signed initial data
    let signed_initial =
        build_sigv4_signed_initial_data("test-data", b"sig-initial-data", 1700000000);
    harness.client.send(signed_initial).await.unwrap();

    // Send signed events
    let signed_event_a = build_sigv4_signed_event_with_signature("A", b"sig-event-A");
    harness.client.send(signed_event_a).await.unwrap();

    let signed_event_b = build_sigv4_signed_event_with_signature("B", b"sig-event-B");
    harness.client.send(signed_event_b).await.unwrap();

    let resp = harness.expect_message().await;
    assert_eq!(get_event_type(&resp), "A");

    // Verify initial data was received
    assert_eq!(harness.server.initial_data(), Some("test-data".to_string()));

    // Verify initial signature
    assert_eq!(
        harness.server.initial_signature(),
        Some(b"sig-initial-data".to_vec())
    );

    // Verify events with signatures
    let events = harness
        .server
        .streaming_operation_with_initial_data_events();
    assert_eq!(events.len(), 2);

    assert_eq!(events[0].message, Events::A(Event {}));
    assert_eq!(
        events[0].signature.as_ref().unwrap().chunk_signature,
        b"sig-event-A"
    );

    assert_eq!(events[1].message, Events::B(Event {}));
    assert_eq!(
        events[1].signature.as_ref().unwrap().chunk_signature,
        b"sig-event-B"
    );
}

/// Test that timestamps are preserved in signatures
#[tokio::test]
async fn test_sigv4_timestamp_preservation() {
    let mut harness = TestHarness::new("StreamingOperation").await;

    // Send events with specific timestamps
    let timestamp1 = 1700000000i64;
    let timestamp2 = 1700000100i64;

    let event1 = sign_message(build_event("A"), b"sig-1", timestamp1);
    harness.client.send(event1).await.unwrap();

    let event2 = sign_message(build_event("B"), b"sig-2", timestamp2);
    harness.client.send(event2).await.unwrap();

    let resp = harness.expect_message().await;
    assert_eq!(get_event_type(&resp), "A");

    let events = harness.server.streaming_operation_events();
    assert_eq!(events.len(), 2);

    // Verify timestamps are preserved
    use std::time::UNIX_EPOCH;

    let expected_time1 = UNIX_EPOCH + std::time::Duration::from_secs(timestamp1 as u64);
    assert_eq!(
        events[0].signature.as_ref().unwrap().timestamp,
        expected_time1
    );

    let expected_time2 = UNIX_EPOCH + std::time::Duration::from_secs(timestamp2 as u64);
    assert_eq!(
        events[1].signature.as_ref().unwrap().timestamp,
        expected_time2
    );
}
