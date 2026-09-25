/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Each generated client speaks one protocol; all of them call the one server serving every
//! protocol, over a real socket.

//! The restXml client makes fewer calls than the others:
//!
//! - The server also serves restJson1, and restJson1 comes first in priority order. A restXml request
//!   whose `Content-Type` does not tell the two REST protocols apart is claimed by restJson1, whose
//!   response the restXml client cannot read. That is every request without a structured body: a
//!   blob upload (`application/octet-stream` for both), an event stream (`application/vnd.amazon.eventstream`
//!   for both), and an input bound only to the URI and headers. So `Upload`, `Subscribe` and `Publish`
//!   are not called over restXml.
//! - The restXml server keeps the legacy wire format for errors: a modeled error is not wrapped in
//!   `<ErrorResponse>`, and a validation error answers with `{}`. The restXml client cannot decode
//!   either, whatever the other protocols, so the error calls are not made over restXml.
//!
//! `Ping` has no input over restXml either; restJson1 answers it, and the empty output reads the same.

/// Calls every client makes: a request carrying every kind of binding, and empty input and output.
macro_rules! core_suite {
    ($module:ident, $sdk:ident) => {
        mod $module {
            use $sdk::{Client, Config};

            async fn client() -> Client {
                let address = multi_protocol::start_server().await;
                Client::from_conf(Config::builder().endpoint_url(format!("http://{address}")).build())
            }

            #[tokio::test]
            async fn greet_round_trips_every_binding() {
                let output = client()
                    .await
                    .greet()
                    .name("ada")
                    .greeting("hi")
                    .times(2)
                    .tags("a")
                    .tags("b")
                    .send()
                    .await
                    .expect("greet");
                assert_eq!(output.message(), "hi ada x2");
                assert_eq!(output.greeted(), Some("ada"));
                assert_eq!(output.tags(), ["a", "b"]);
            }

            #[tokio::test]
            async fn ping_has_empty_input_and_output() {
                client().await.ping().send().await.expect("ping");
            }
        }
    };
}

/// Modeled and validation errors, and event streams in both directions.
macro_rules! error_and_stream_suite {
    ($module:ident, $sdk:ident) => {
        mod $module {
            use $sdk::operation::greet::GreetError;
            use $sdk::types::error::EventsError;
            use $sdk::types::{Events, Note};
            use $sdk::{Client, Config};

            async fn client() -> Client {
                let address = multi_protocol::start_server().await;
                Client::from_conf(Config::builder().endpoint_url(format!("http://{address}")).build())
            }

            #[tokio::test]
            async fn greet_returns_the_modeled_error() {
                let error = client().await.greet().name("intruder").send().await.unwrap_err();
                match error.into_service_error() {
                    GreetError::Unwelcome(unwelcome) => {
                        assert_eq!(unwelcome.message(), "go away");
                        assert_eq!(unwelcome.who(), Some("intruder"));
                    }
                    other => panic!("expected Unwelcome, got {other:?}"),
                }
            }

            #[tokio::test]
            async fn greet_returns_the_validation_error() {
                let error = client().await.greet().name("x".repeat(33)).send().await.unwrap_err();
                match error.into_service_error() {
                    GreetError::ValidationError(validation) => {
                        assert!(validation.message().contains("/name"), "{validation:?}");
                    }
                    other => panic!("expected ValidationError, got {other:?}"),
                }
            }

            #[tokio::test]
            async fn subscribe_receives_the_server_stream() {
                let mut output = client()
                    .await
                    .subscribe()
                    .topic("news")
                    .count(3)
                    .send()
                    .await
                    .expect("subscribe");
                let mut texts = Vec::new();
                while let Some(event) = output.events.recv().await.expect("event") {
                    let Events::Note(note) = event else {
                        panic!("unexpected event {event:?}")
                    };
                    texts.push(note.text.unwrap_or_default());
                }
                assert_eq!(texts, ["news #0", "news #1", "news #2"]);
            }

            #[tokio::test]
            async fn publish_sends_the_client_stream() {
                let notes = ["a", "b", "c"]
                    .map(|text| Ok::<_, EventsError>(Events::Note(Note::builder().text(text).build())));
                let output = client()
                    .await
                    .publish()
                    .topic("news")
                    .events(futures_util::stream::iter(notes).into())
                    .send()
                    .await
                    .expect("publish");
                assert_eq!(output.received(), 3);
                assert_eq!(output.text(), "news: a|b|c");
            }
        }
    };
}

mod rpcv2_cbor {
    core_suite!(core, multi_protocol_client_rpcv2cbor);
    error_and_stream_suite!(errors_and_streams, multi_protocol_client_rpcv2cbor);
}

mod aws_json_1_0 {
    core_suite!(core, multi_protocol_client_awsjson10);
    error_and_stream_suite!(errors_and_streams, multi_protocol_client_awsjson10);
}

mod aws_json_1_1 {
    core_suite!(core, multi_protocol_client_awsjson11);
    error_and_stream_suite!(errors_and_streams, multi_protocol_client_awsjson11);
}

mod rest_json_1 {
    core_suite!(core, multi_protocol_client_restjson);
    error_and_stream_suite!(errors_and_streams, multi_protocol_client_restjson);

    /// A streaming blob upload, which only the REST protocols model.
    #[tokio::test]
    async fn upload_streams_the_blob() {
        use multi_protocol_client_restjson::primitives::ByteStream;
        use multi_protocol_client_restjson::{Client, Config};
        let address = multi_protocol::start_server().await;
        let client = Client::from_conf(Config::builder().endpoint_url(format!("http://{address}")).build());
        let output = client
            .upload()
            .data(ByteStream::from_static(b"streamed payload"))
            .send()
            .await
            .expect("upload");
        assert_eq!(output.size(), 16);
    }
}

mod rest_xml {
    core_suite!(core, multi_protocol_client_restxml);
}
