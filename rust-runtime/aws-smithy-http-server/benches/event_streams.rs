/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Schema event-stream runtime benchmarks using a one-time copy of generated fixtures.
//! No generated SDK or codegen step is needed; see fixtures/event_stream/README.md.

#[path = "fixtures/event_stream/error.rs"]
mod error;
#[path = "fixtures/event_stream/event_stream_serde.rs"]
mod event_stream_serde;
#[path = "fixtures/event_stream/model.rs"]
mod model;
#[path = "fixtures/event_stream/output.rs"]
mod output;

use aws_smithy_eventstream::frame::{
    read_message_from, write_message_to, MarshallMessage, UnmarshallMessage, UnmarshalledMessage,
};
use aws_smithy_http::event_stream::EventStreamSender;
use aws_smithy_http_server::body::BoxBody;
use aws_smithy_http_server::schema::event_stream::{event_stream_response_body, InitialResponsePolicy};
use aws_smithy_http_server::schema::protocol::{RestJson1, RpcV2Cbor};
use aws_smithy_http_server::schema::SharedServerProtocol;
use aws_smithy_types::event_stream::{HeaderValue, Message};
use aws_smithy_types::Blob;
use bytes::Bytes;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use event_stream_serde::{EventsErrorMarshaller, EventsMarshaller, EventsUnmarshaller};
use http_body_util::BodyExt;
use model::{BinaryEvent, Events, Nested, StructuredData, StructuredEvent};
use std::hint::black_box;

const METADATA: &str = "event-stream-benchmark-v1";
const STREAM_EVENTS: usize = 64;

type Sender = EventStreamSender<Events, error::EventsError>;

fn event(kind: &str, size: usize) -> Events {
    match kind {
        "structured" => Events::Structured(StructuredEvent {
            sequence: Some(42),
            payload: Some(StructuredData {
                text: Some("x".repeat(size)),
                nested: Some(Nested {
                    label: Some(METADATA.into()),
                    enabled: Some(true),
                }),
                values: Some((0..8).collect()),
            }),
        }),
        "binary" => Events::Binary(BinaryEvent {
            sequence: Some(42),
            payload: Some(Blob::new(vec![42; size])),
        }),
        _ => unreachable!(),
    }
}

fn header<'a>(message: &'a Message, name: &str) -> &'a HeaderValue {
    message
        .headers()
        .iter()
        .find(|header| header.name().as_str() == name)
        .unwrap_or_else(|| panic!("missing {name} header"))
        .value()
}

fn string_header<'a>(message: &'a Message, name: &str) -> &'a str {
    header(message, name).as_string().unwrap().as_str()
}

fn encode(message: &Message) -> Bytes {
    let mut bytes = Vec::new();
    write_message_to(message, &mut bytes).expect("encode frame with CRCs");
    Bytes::from(bytes)
}

fn validate_event(protocol: &SharedServerProtocol, expected: &Events, kind: &str, size: usize, message: &Message) {
    assert_eq!(string_header(message, ":message-type"), "event");
    assert_eq!(string_header(message, ":event-type"), kind);
    assert_eq!(header(message, "sequence").as_int64().unwrap(), 42);
    assert_eq!(message.headers().len(), 4);
    if kind == "binary" {
        assert_eq!(string_header(message, ":content-type"), "application/octet-stream");
        assert_eq!(message.payload(), vec![42; size].as_slice());
    } else {
        assert_eq!(
            string_header(message, ":content-type"),
            protocol.event_stream_framing().unwrap().media_type
        );
    }
    match EventsUnmarshaller::new(protocol.clone()).unmarshall(message).unwrap() {
        UnmarshalledMessage::Event(actual) => assert_eq!(&actual, expected),
        _ => panic!("expected an event"),
    }
    let mut bytes = encode(message);
    let decoded = read_message_from(&mut bytes).expect("validate prelude and message CRCs");
    assert_eq!(decoded, *message);
    assert!(bytes.is_empty());
}

fn bench_event(criterion: &mut Criterion, protocol: &SharedServerProtocol, name: &str, kind: &str) {
    let mut group = criterion.benchmark_group(format!("event_streams/{name}/{kind}"));
    group.throughput(Throughput::Elements(1));
    for size in [64, 1024, 65536] {
        let fixture = event(kind, size);
        let marshaller = EventsMarshaller::new(protocol.clone());
        // Preflight also warms the marshaller's per-variant header cache.
        let message = marshaller.marshall(fixture.clone()).unwrap();
        validate_event(protocol, &fixture, kind, size, &message);
        let bytes = encode(&message);
        let unmarshaller = EventsUnmarshaller::new(protocol.clone());

        group.bench_function(BenchmarkId::new("serialize_event", size), |bencher| {
            bencher.iter_batched(
                || fixture.clone(),
                |event| black_box(marshaller.marshall(black_box(event)).unwrap()),
                BatchSize::SmallInput,
            );
        });
        group.bench_function(BenchmarkId::new("serialize_first_event", size), |bencher| {
            bencher.iter_batched(
                || fixture.clone(),
                |event| {
                    black_box(
                        EventsMarshaller::new(protocol.clone())
                            .marshall(black_box(event))
                            .unwrap(),
                    )
                },
                BatchSize::SmallInput,
            );
        });
        group.bench_function(BenchmarkId::new("deserialize_event", size), |bencher| {
            bencher.iter(|| black_box(unmarshaller.unmarshall(black_box(&message)).unwrap()));
        });
        group.bench_function(BenchmarkId::new("encode_frame", size), |bencher| {
            bencher.iter_batched(
                || fixture.clone(),
                |event| {
                    let message = marshaller.marshall(black_box(event)).unwrap();
                    black_box(encode(&message))
                },
                BatchSize::SmallInput,
            );
        });
        group.bench_function(BenchmarkId::new("decode_frame", size), |bencher| {
            bencher.iter(|| {
                let message = read_message_from(black_box(bytes.as_ref())).unwrap();
                black_box(unmarshaller.unmarshall(&message).unwrap())
            });
        });
    }
    group.finish();
}

fn exception() -> error::EventsError {
    error::EventsError::StreamFailure(error::StreamFailure {
        message: Some("expected stream failure".into()),
    })
}

fn validate_exception(unmarshaller: &EventsUnmarshaller, message: &Message) {
    assert_eq!(string_header(message, ":message-type"), "exception");
    assert_eq!(string_header(message, ":exception-type"), "failure");
    match unmarshaller.unmarshall(message).unwrap() {
        UnmarshalledMessage::Error(error::EventsError::StreamFailure(error)) => {
            assert_eq!(error.message.as_deref(), Some("expected stream failure"));
        }
        _ => panic!("expected a modeled stream error"),
    }
}

fn bench_exception(criterion: &mut Criterion, protocol: &SharedServerProtocol, name: &str) {
    let marshaller = EventsErrorMarshaller::new(protocol.clone());
    let unmarshaller = EventsUnmarshaller::new(protocol.clone());
    let message = marshaller.marshall(exception()).unwrap();
    validate_exception(&unmarshaller, &message);
    let bytes = encode(&message);
    let decoded = read_message_from(bytes.as_ref()).unwrap();
    assert_eq!(decoded, message);
    validate_exception(&unmarshaller, &decoded);
    let mut group = criterion.benchmark_group(format!("event_streams/{name}/exception"));
    group.throughput(Throughput::Elements(1));
    group.bench_function("serialize_error", |bencher| {
        bencher.iter_batched(
            exception,
            |error| black_box(marshaller.marshall(black_box(error)).unwrap()),
            BatchSize::SmallInput,
        );
    });
    group.bench_function("deserialize_error", |bencher| {
        bencher.iter(|| black_box(unmarshaller.unmarshall(black_box(&message)).unwrap()));
    });
    group.bench_function("encode_frame", |bencher| {
        bencher.iter_batched(
            exception,
            |error| black_box(encode(&marshaller.marshall(black_box(error)).unwrap())),
            BatchSize::SmallInput,
        );
    });
    group.bench_function("decode_frame", |bencher| {
        bencher.iter(|| {
            let message = read_message_from(black_box(bytes.as_ref())).unwrap();
            black_box(unmarshaller.unmarshall(&message).unwrap())
        });
    });
    group.finish();
}

fn sender(fixture: &Events) -> Sender {
    futures_util::stream::iter((0..STREAM_EVENTS).map(|_| Ok(fixture.clone())).collect::<Vec<_>>()).into()
}

fn response_body(protocol: &SharedServerProtocol, output: &output::DownloadOutput, sender: Sender) -> BoxBody {
    event_stream_response_body(
        output::DownloadOutput::SCHEMA,
        output,
        sender,
        EventsMarshaller::new(protocol.clone()),
        EventsErrorMarshaller::new(protocol.clone()),
        protocol,
        InitialResponsePolicy::Send,
    )
    .unwrap()
}

fn bench_response_body(criterion: &mut Criterion, protocol: &SharedServerProtocol, name: &str) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let output = output::DownloadOutput {
        metadata: METADATA.into(),
        events: futures_util::stream::empty().into(),
    };
    let mut group = criterion.benchmark_group(format!("event_streams/{name}/response_body"));
    group.throughput(Throughput::Elements(STREAM_EVENTS as u64));
    for kind in ["structured", "binary"] {
        let fixture = event(kind, 1024);
        // Deep checks and frame parsing stay outside the measured response-body drain.
        runtime.block_on(async {
            let bytes = response_body(protocol, &output, sender(&fixture))
                .collect()
                .await
                .unwrap()
                .to_bytes();
            let mut remaining = bytes;
            if protocol.event_stream_framing().unwrap().initial_messages_in_frames {
                let initial = read_message_from(&mut remaining).unwrap();
                assert_eq!(string_header(&initial, ":event-type"), "initial-response");
                let mut deser = protocol
                    .event_stream_framing()
                    .unwrap()
                    .payload_codec
                    .create_deserializer(initial.payload());
                let mut metadata = None;
                deser
                    .read_struct(output::DownloadOutput::SCHEMA, &mut |member, deser| {
                        if member.member_name() == Some("metadata") {
                            metadata = Some(deser.read_string(member)?);
                        }
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(metadata.as_deref(), Some(METADATA));
            }
            for _ in 0..STREAM_EVENTS {
                let message = read_message_from(&mut remaining).unwrap();
                validate_event(protocol, &fixture, kind, 1024, &message);
            }
            assert!(remaining.is_empty());
        });
        group.bench_function(BenchmarkId::new(kind, "1024B-64events"), |bencher| {
            bencher.iter_batched(
                || sender(&fixture),
                |sender| {
                    runtime.block_on(async {
                        let mut body = response_body(protocol, &output, sender);
                        let mut frames = 0;
                        let mut bytes = 0;
                        while let Some(frame) = body.frame().await {
                            if let Ok(data) = frame.unwrap().into_data() {
                                frames += 1;
                                bytes += data.len();
                            }
                        }
                        black_box((frames, bytes))
                    })
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

fn event_streams(criterion: &mut Criterion) {
    for (name, protocol) in [
        (
            "rest_json_1",
            SharedServerProtocol::metadata_routed(RestJson1::default()),
        ),
        (
            "rpc_v2_cbor",
            SharedServerProtocol::metadata_routed(RpcV2Cbor::default()),
        ),
    ] {
        for kind in ["structured", "binary"] {
            bench_event(criterion, &protocol, name, kind);
        }
        bench_exception(criterion, &protocol, name);
        bench_response_body(criterion, &protocol, name);
    }
}

criterion_group!(benches, event_streams);
criterion_main!(benches);
