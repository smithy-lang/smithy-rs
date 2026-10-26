/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Implements one generated service serving all five supported protocols through
//! shared handlers. It returns predictable success, validation, modeled-error, blob,
//! and event-stream values and can start a socket server for generated client tests.
//!
//! A `MultiProtocolService` serving rpcv2Cbor, awsJson1.0, awsJson1.1, restJson1 and restXml from one
//! set of handlers.

use std::net::SocketAddr;

use aws_smithy_types::{Blob, DateTime};
use multi_protocol_server_sdk::error::{
    GreetError, PublishError, RichDataError, RichFailure, StructurePayloadError, SubscribeError,
    UnionPayloadError, Unwelcome, UploadError,
};
use multi_protocol_server_sdk::input::{
    GreetInput, ModeledDefaultsInput, NoInputOutputInput, NullableValuesInput, PingInput, PublishInput,
    ResponseBindingsInput, RichDataInput, StructurePayloadInput, SubscribeInput, UnionPayloadInput,
    UploadInput,
};
use multi_protocol_server_sdk::model::{
    DefaultColor, Events, NestedRecord, Note, PayloadChoice, PayloadRecord, RichRecord,
};
use multi_protocol_server_sdk::output::{
    GreetOutput, ModeledDefaultsOutput, NoInputOutputOutput, NullableValuesOutput, PingOutput, PublishOutput,
    ResponseBindingsOutput, RichDataOutput, StructurePayloadOutput, SubscribeOutput,
    UnionPayloadOutput, UploadOutput,
};
use multi_protocol_server_sdk::{MultiProtocolService, MultiProtocolServiceConfig};

/// Greets `name`, echoing every binding back; `intruder` is turned away with a modeled error.
pub async fn greet(input: GreetInput) -> Result<GreetOutput, GreetError> {
    let name = input.name.into_inner();
    if name == "intruder" {
        return Err(GreetError::Unwelcome(Unwelcome {
            message: "go away".to_owned(),
            who: Some(name),
        }));
    }
    let greeting = input.greeting.unwrap_or_else(|| "hello".to_owned());
    let times = input.times.unwrap_or(1);
    Ok(GreetOutput {
        message: format!("{greeting} {name} x{times}"),
        greeted: Some(name),
        tags: input.tags,
    })
}

pub async fn no_input_output(_: NoInputOutputInput) -> NoInputOutputOutput {
    NoInputOutputOutput {}
}

pub async fn ping(_: PingInput) -> PingOutput {
    PingOutput {}
}

/// Answers with the number of bytes streamed in.
pub async fn upload(input: UploadInput) -> Result<UploadOutput, UploadError> {
    let bytes = input
        .data
        .collect()
        .await
        .expect("upload body")
        .into_bytes();
    Ok(UploadOutput {
        size: bytes.len() as i64,
    })
}

/// Streams `count` notes on `topic`.
pub async fn subscribe(input: SubscribeInput) -> Result<SubscribeOutput, SubscribeError> {
    let topic = input.topic;
    let notes = (0..input.count).map(move |index| {
        Ok(Events::Note(Note {
            text: Some(format!("{topic} #{index}")),
        }))
    });
    Ok(SubscribeOutput {
        events: futures_util::stream::iter(notes).into(),
    })
}

/// Counts the notes streamed in and joins their text.
pub async fn publish(mut input: PublishInput) -> Result<PublishOutput, PublishError> {
    let mut texts = Vec::new();
    while let Some(event) = input.events.recv().await.expect("publish events") {
        let Events::Note(note) = event;
        texts.push(note.text.unwrap_or_default());
    }
    Ok(PublishOutput {
        received: texts.len() as i32,
        text: format!("{}: {}", input.topic, texts.join("|")),
    })
}

/// Echoes an ordinary structure payload, including its constrained and nested members.
pub async fn structure_payload(
    input: StructurePayloadInput,
) -> Result<StructurePayloadOutput, StructurePayloadError> {
    Ok(StructurePayloadOutput { body: input.body })
}

/// Echoes either variant of an ordinary union payload.
pub async fn union_payload(
    input: UnionPayloadInput,
) -> Result<UnionPayloadOutput, UnionPayloadError> {
    Ok(UnionPayloadOutput { body: input.body })
}

/// Uses REST bindings deliberately: RPC must serialize the members as document fields.
pub async fn response_bindings(input: ResponseBindingsInput) -> ResponseBindingsOutput {
    let scenario = input.scenario.as_deref().unwrap_or("populated");
    ResponseBindingsOutput {
        name: Some("n".to_owned()),
        etag: Some("\"abc\"".to_owned()),
        metadata: (scenario != "absent").then(|| {
            [
                ("one".to_owned(), "1".to_owned()),
                ("two".to_owned(), "2".to_owned()),
            ]
            .into_iter()
            .collect()
        }),
        code: match scenario {
            "absent" => None,
            "override" => Some(202),
            _ => Some(299),
        },
    }
}

fn nested_record(name: &str, count: i32, active: Option<bool>) -> NestedRecord {
    NestedRecord {
        name: name.to_owned(),
        count: Some(count),
        active,
    }
}

fn populated_payload() -> PayloadRecord {
    PayloadRecord {
        label: "fixture"
            .to_owned()
            .try_into()
            .expect("valid fixture label"),
        child: Some(nested_record("child", 7, Some(true))),
        items: Some(vec![nested_record("first", 1, Some(false))]),
        lookup: Some(
            [("key".to_owned(), nested_record("mapped", 2, None))]
                .into_iter()
                .collect(),
        ),
        choice: Some(PayloadChoice::Text("chosen".to_owned())),
        bytes: Some(Blob::new(b"abc")),
        timestamp: Some(DateTime::from_secs(0)),
    }
}

/// Exercises nested values on success and populated error bodies and bindings on failure.
pub async fn rich_data(input: RichDataInput) -> Result<RichDataOutput, RichDataError> {
    if input.scenario.as_deref() == Some("error") {
        return Err(RichDataError::RichFailure(RichFailure {
            message: "rejected".to_owned(),
            details: Some(nested_record("diagnostic", 9, Some(false))),
            etag: Some("error-tag".to_owned()),
            metadata: Some(
                [("reason".to_owned(), "fixture".to_owned())]
                    .into_iter()
                    .collect(),
            ),
        }));
    }
    Ok(RichDataOutput {
        record: input.record.unwrap_or_else(|| RichRecord {
            payload: populated_payload(),
            children: Some(vec![nested_record("extra", 9, None)]),
            choices: Some(vec![
                PayloadChoice::Text("list-choice".to_owned()),
                PayloadChoice::Record(nested_record("union-record", 3, None)),
            ]),
            blobs: Some(vec![Blob::new(b"abc"), Blob::new(b"xyz")]),
        }),
    })
}

/// Leaves modeled defaults to the generated builder, or explicitly overrides all of them.
pub async fn modeled_defaults(input: ModeledDefaultsInput) -> ModeledDefaultsOutput {
    let builder = ModeledDefaultsOutput::builder();
    if input.scenario.as_deref() == Some("override") {
        builder
            .text("explicit".to_owned())
            .count(23)
            .color(DefaultColor::Blue)
            .build()
    } else {
        builder.build()
    }
}

/// Separates omitted members, empty containers, and explicit null entries in sparse collections.
pub async fn nullable_values(input: NullableValuesInput) -> NullableValuesOutput {
    match input.scenario.as_deref().unwrap_or("populated") {
        "absent" => NullableValuesOutput {
            values: None,
            entries: None,
        },
        "empty" => NullableValuesOutput {
            values: Some(vec![]),
            entries: Some(Default::default()),
        },
        _ => NullableValuesOutput {
            values: Some(vec![
                Some("first".to_owned()),
                None,
                Some("last".to_owned()),
            ]),
            entries: Some(
                [
                    ("one".to_owned(), Some("value".to_owned())),
                    ("nil".to_owned(), None),
                ]
                .into_iter()
                .collect(),
            ),
        },
    }
}

pub fn service() -> MultiProtocolService {
    MultiProtocolService::builder(MultiProtocolServiceConfig::builder().build())
        .greet(greet)
        .ping(ping)
        .no_input_output(no_input_output)
        .upload(upload)
        .subscribe(subscribe)
        .publish(publish)
        .structure_payload(structure_payload)
        .union_payload(union_payload)
        .response_bindings(response_bindings)
        .rich_data(rich_data)
        .modeled_defaults(modeled_defaults)
        .nullable_values(nullable_values)
        .build()
        .expect("every operation has a handler")
}

/// Serves the service on an ephemeral local port, returning its address.
pub async fn start_server() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("local address");
    tokio::spawn(async move {
        multi_protocol_server_sdk::serve(listener, service().into_make_service())
            .await
            .expect("server");
    });
    address
}
