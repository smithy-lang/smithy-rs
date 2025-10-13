/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use aws_smithy_http::event_stream::EventStreamSender;
use bytes::Bytes;
use eventstreams::ManualEventStreamClient;
use rpcv2cbor_extras::error::{StreamingOperationError, ValidationException};
use rpcv2cbor_extras::model::{Event, Events};
use rpcv2cbor_extras::{error, input, output, RpcV2CborService, RpcV2CborServiceConfig};
use tokio::net::TcpListener;

async fn streaming_operation(
    mut input: input::StreamingOperationInput,
) -> Result<output::StreamingOperationOutput, error::StreamingOperationError> {
    // Minimal handler - just consume the input stream and return empty output
    let ev = input.events.recv().await.map_err(|err| {
        StreamingOperationError::ValidationException(
            ValidationException::builder()
                .message(format!("{:?}", err))
                .build()
                .unwrap(),
        )
    });
    let resp = if dbg!(&ev).is_err() {
        Events::B(Event {})
    } else {
        Events::A(Event {})
    };
    println!("SERVER EVENT: {:#?}", ev);

    Ok(output::StreamingOperationOutput::builder()
        .events(EventStreamSender::once(Ok(resp)))
        .build()
        .unwrap())
}

#[tokio::test]
async fn test_initial_request_handling() {
    //let _guard = show_test_logs();
    let config = RpcV2CborServiceConfig::builder().build();
    let app = RpcV2CborService::builder::<hyper0::Body, _, _, _>(config)
        .streaming_operation(streaming_operation)
        .build_unchecked();

    // Bind server to local port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Start server
    tokio::spawn(async move {
        let make_service = app.into_make_service();
        let server = hyper0::Server::from_tcp(listener.into_std().unwrap())
            .unwrap()
            .http2_only(true)
            .serve(make_service);
        server.await.unwrap();
    });

    // Connect with ManualEventStreamClient
    let path = "/service/RpcV2CborService/operation/StreamingOperation";
    let mut client = ManualEventStreamClient::connect_to_service(
        addr,
        path,
        vec![("Smithy-Protocol", "rpc-v2-cbor")],
    )
    .await
    .unwrap();

    // Send initial-request event
    use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
    let headers = vec![
        Header::new(":message-type", HeaderValue::String("event".into())),
        Header::new(":event-type", HeaderValue::String("initial-request".into())),
        Header::new(
            ":content-type",
            HeaderValue::String("application/cbor".into()),
        ),
    ];
    let empty_cbor_payload = Bytes::from(vec![0xbf, 0xff]);
    let initial_request = Message::new_from_parts(headers, empty_cbor_payload.clone()); // Empty CBOR map
    let _response = client.send(initial_request).await;
    println!("main response: {:?}", _response);
    client
        .send(Message::new_from_parts(
            vec![
                Header::new(":message-type", HeaderValue::String("event".into())),
                Header::new(
                    ":content-type",
                    HeaderValue::String("application/cbor".into()),
                ),
                Header::new(":event-type", HeaderValue::String("A".into())),
            ],
            empty_cbor_payload,
        ))
        .await
        .unwrap();
    let resp = client.recv().await.unwrap().unwrap();
    let event_type = resp
        .headers()
        .iter()
        .find(|h| h.name().as_str() == ":event-type")
        .expect("no event type header");
    println!("{:#?}", resp);
    // THIS TEST DOCUMENTS CURRENT BROKEN BEHAVIOR
    // The server should handle an initial-request but currently it does not.
    // If this returns "A" it means we successfully parsed the request.
    // If this returns "B" it means we failed to parse the request.
    assert_eq!(event_type.value().as_string().unwrap().as_str(), "B");
}
