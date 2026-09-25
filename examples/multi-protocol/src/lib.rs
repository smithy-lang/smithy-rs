/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A `MultiProtocolService` serving rpcv2Cbor, awsJson1.0, awsJson1.1, restJson1 and restXml from one
//! set of handlers.

use std::net::SocketAddr;

use multi_protocol_server_sdk::error::{GreetError, PublishError, SubscribeError, UploadError, Unwelcome};
use multi_protocol_server_sdk::input::{GreetInput, PingInput, PublishInput, SubscribeInput, UploadInput};
use multi_protocol_server_sdk::model::{Events, Note};
use multi_protocol_server_sdk::output::{GreetOutput, PingOutput, PublishOutput, SubscribeOutput, UploadOutput};
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

pub async fn ping(_: PingInput) -> PingOutput {
    PingOutput {}
}

/// Answers with the number of bytes streamed in.
pub async fn upload(input: UploadInput) -> Result<UploadOutput, UploadError> {
    let bytes = input.data.collect().await.expect("upload body").into_bytes();
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

pub fn service() -> MultiProtocolService {
    MultiProtocolService::builder(MultiProtocolServiceConfig::builder().build())
        .greet(greet)
        .ping(ping)
        .upload(upload)
        .subscribe(subscribe)
        .publish(publish)
        .build()
        .expect("every operation has a handler")
}

/// Serves the service on an ephemeral local port, returning its address.
pub async fn start_server() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("local address");
    tokio::spawn(async move {
        multi_protocol_server_sdk::serve(listener, service().into_make_service())
            .await
            .expect("server");
    });
    address
}
