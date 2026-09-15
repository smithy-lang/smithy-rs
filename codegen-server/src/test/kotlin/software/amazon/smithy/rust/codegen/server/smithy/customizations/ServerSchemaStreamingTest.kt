/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenConfig
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * A duplex event stream on the schema path: frames go in through the streaming upgrade, the handler echoes
 * them, frames come out through the streaming glue. On rpcv2Cbor the non-stream members travel in
 * `initial-request` and `initial-response` frames; on restJson1 in the URI and headers.
 */
internal class ServerSchemaStreamingTest {
    private fun model(rpc: Boolean): String {
        val service = if (rpc) "@rpcv2Cbor" else "@restJson1"
        val http = if (rpc) "" else "@http(uri: \"/chat/{room}\", method: \"POST\")"
        val label = if (rpc) "" else "@httpLabel"
        val nickHeader = if (rpc) "" else "@httpHeader(\"x-nick\")"
        val topicHeader = if (rpc) "" else "@httpHeader(\"x-topic\")"
        val payload = if (rpc) "" else "@httpPayload"
        return """
            ${'$'}version: "2"
            namespace com.aws.example.streaming

            use aws.protocols#restJson1
            use smithy.protocols#rpcv2Cbor

            $service
            service ChatService {
                version: "2026-09-12"
                operations: [Chat, Upload, Download${if (rpc) "" else ", BlobEcho"}]
            }

            $http
            operation Chat {
                input := {
                    @required
                    $label
                    room: String
                    $nickHeader
                    nick: String
                    $payload
                    events: ChatEvents
                }
                output := {
                    $topicHeader
                    topic: String
                    $payload
                    events: ChatEvents
                }
            }

            ${if (rpc) "" else "@http(uri: \"/upload\", method: \"POST\")"}
            operation Upload {
                input := { $payload events: ChatEvents }
                output := { count: Integer }
                errors: [smithy.framework#ValidationException]
            }
            ${if (rpc) "" else "@http(uri: \"/download\", method: \"POST\")"}
            operation Download {
                input := {}
                output := { $payload events: ChatEvents }
                errors: [smithy.framework#ValidationException]
            }

            ${if (rpc) {
            ""
        } else {
            """
                @http(uri: "/blob", method: "POST")
                operation BlobEcho {
                    input := { @required @httpPayload data: Data }
                    output := { @required @httpPayload data: Data }
                    errors: [smithy.framework#ValidationException]
                }
                @streaming
                blob Data
            """
        }}

            structure ImplicitEvent {
                @eventHeader
                from: String
                value: Kind
            }

            @streaming
            union ChatEvents {
                implicit: ImplicitEvent
                message: ChatMessage
                bye: Bye
                text: TextEvent
                binary: BinaryEvent
                checked: CheckedEvent
                failure: StreamFailure
            }

            structure ChatMessage {
                @eventHeader
                from: String
                @eventPayload
                body: MessageBody
            }

            structure MessageBody {
                text: String
            }

            structure Bye {}
            structure TextEvent { @eventPayload value: String }
            structure BinaryEvent { @eventPayload value: Blob }
            structure CheckedEvent { @eventPayload value: CheckedBody }
            structure CheckedBody { value: Kind }
            enum Kind {
                VALID = "valid"
            }
            @error("server")
            structure StreamFailure { message: String }
            """
    }

    private fun params(
        rpc: Boolean,
        sendInitial: Boolean = rpc,
    ): IntegrationTestParams {
        val codegen =
            ObjectNode.builder()
                .withMember(ServerCodegenConfig.SCHEMA_SERDE_CONFIG_KEY, true)
                .withMember("alwaysSendEventStreamInitialResponse", sendInitial)
                .build()
        return IntegrationTestParams(
            additionalSettings = ObjectNode.builder().withMember("codegen", codegen).build(),
        )
    }

    private fun scope(codegenContext: ServerCodegenContext) =
        arrayOf(
            *preludeScope,
            "Cbor" to CargoDependency.smithyCbor(codegenContext.runtimeConfig).toType(),
            "RuntimeApi" to RuntimeType.smithyRuntimeApi(codegenContext.runtimeConfig),
            "Bytes" to RuntimeType.Bytes,
            "Schema" to RuntimeType.smithySchema(codegenContext.runtimeConfig),
            "FuturesUtil" to ServerCargoDependency.FuturesUtil.toType(),
            "Http" to RuntimeType.http(codegenContext.runtimeConfig),
            "HttpBody" to CargoDependency.HttpBody1x.toType(),
            "HttpBodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
            "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
            "SmithyEventStream" to RuntimeType.smithyEventStream(codegenContext.runtimeConfig),
            "SmithyHttp" to RuntimeType.smithyHttp(codegenContext.runtimeConfig),
            "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
            "SmithyTypes" to RuntimeType.smithyTypes(codegenContext.runtimeConfig),
            "Tower" to ServerCargoDependency.Tower.toType(),
        )

    /** The service under test: every incoming event is echoed, then `Bye` closes the stream. */
    private val echoService =
        """
        let config = crate::service::ChatServiceConfig::builder().build();
        let service = crate::service::ChatService::builder(config)
            .chat(|mut input: crate::input::ChatInput| async move {
                let topic = format!("{}/{}", input.room, input.nick.unwrap_or_default());
                let mut echoed = #{Vec}::new();
                while let #{Some}(event) = input.events.recv().await.expect("a well-formed frame") {
                    echoed.push(#{Ok}(event));
                }
                echoed.push(#{Ok}(crate::model::ChatEvents::Bye(crate::model::Bye {})));
                #{Ok}::<_, crate::error::ChatError>(crate::output::ChatOutput {
                    topic: #{Some}(topic),
                    events: #{FuturesUtil}::stream::iter(echoed).into(),
                })
            })
            .build_unchecked();
        """

    /** Two frames marshalled through the generated schema-mode marshaller. */
    private val requestFrames =
        """
        use #{SmithyEventStream}::frame::MarshallMessage;
        let marshaller = crate::event_stream_serde::ChatEventsMarshaller::new(protocol.clone());
        let mut body = #{Vec}::new();
        for text in ["hi", "there"] {
            let event = crate::model::ChatEvents::Message(crate::model::ChatMessage {
                from: #{Some}("ann".to_owned()),
                body: #{Some}(crate::model::MessageBody { text: #{Some}(text.to_owned()) }),
            });
            let message = marshaller.marshall(event).expect("marshalls");
            #{SmithyEventStream}::frame::write_message_to(&message, &mut body).expect("writes");
        }
        """

    /** Reads the echoed frames back through the generated schema-mode unmarshaller. */
    private val responseFrames =
        """
        assert_eq!(response.status(), #{Http}::StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/vnd.amazon.eventstream"
        );
        use #{HttpBodyUtil}::BodyExt;
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.expect("collects").to_bytes();
        // Decode transport frames independently of the generated union unmarshaller.
        let mut raw = bytes.clone();
        let mut frames = #{Vec}::new();
        while !raw.is_empty() {
            frames.push(#{SmithyEventStream}::frame::read_message_from(&mut raw).expect("valid frame and CRC"));
        }
        let event_types: #{Vec}<_> = frames.iter().map(|frame| {
            frame.headers().iter().find(|header| header.name().as_str() == ":event-type")
                .unwrap().value().as_string().unwrap().as_str()
        }).collect();
        let initial = event_types.first() == #{Some}(&"initial-response");
        assert_eq!(&event_types[usize::from(initial)..], &["message", "message", "bye"]);
        assert_eq!(frames[usize::from(initial)].headers().iter().find(|header| header.name().as_str() == "from")
            .unwrap().value().as_string().unwrap().as_str(), "ann");
        for (frame, text) in frames[usize::from(initial)..].iter().zip(["hi", "there"]) {
            let content_type = frame.headers().iter().find(|h| h.name().as_str() == ":content-type").unwrap()
                .value().as_string().unwrap().as_str();
            assert_eq!(content_type, protocol.event_stream().unwrap().event_stream_media_type());
            if content_type == "application/cbor" {
                let mut decoder = #{Cbor}::Decoder::new(frame.payload());
                decoder.map().unwrap();
                assert_eq!(decoder.str().unwrap(), "text");
                assert_eq!(decoder.str().unwrap(), text);
            } else {
                assert_eq!(std::str::from_utf8(frame.payload()).unwrap(), format!("{{\"text\":\"{text}\"}}"));
            }
        }
        let unmarshaller = crate::event_stream_serde::ChatEventsUnmarshaller::new(protocol.clone());
        let mut receiver = #{SmithyHttp}::event_stream::Receiver::new(unmarshaller, #{SdkBody}::from(bytes));
        """

    private val echoedEvents =
        """
        for text in ["hi", "there"] {
            match receiver.recv().await.expect("a frame").expect("an event") {
                crate::model::ChatEvents::Message(message) => {
                    assert_eq!(message.from.as_deref(), #{Some}("ann"));
                    assert_eq!(message.body.and_then(|body| body.text).as_deref(), #{Some}(text));
                }
                other => panic!("unexpected event {other:?}"),
            }
        }
        assert!(matches!(
            receiver.recv().await.expect("a frame").expect("an event"),
            crate::model::ChatEvents::Bye(_)
        ));
        assert!(receiver.recv().await.expect("end of stream").is_none());
        """

    private val frameTests =
        """
        use #{SmithyEventStream}::frame::{MarshallMessage, UnmarshallMessage, UnmarshalledMessage};
        use crate::event_stream_serde::{ChatEventsMarshaller, ChatEventsUnmarshaller, ChatEventsErrorMarshaller};
        use crate::model::{ChatEvents, TextEvent, BinaryEvent};
        use #{SmithyHttpServer}::schema::SharedServerProtocol;
        // The very same generated types must follow either selected runtime codec.
        for (protocol, content_type, empty) in [
            (SharedServerProtocol::new(#{SmithyHttpServer}::protocol::rest_json_1::RestJson1Protocol::default()), "application/json", br##"{"value":"bad"}"##.as_slice()),
            (SharedServerProtocol::new(#{SmithyHttpServer}::protocol::rpc_v2_cbor::RpcV2CborProtocol::default()), "application/cbor", b"\xa1\x65value\x63bad".as_slice()),
        ] {
            let marshaller = ChatEventsMarshaller::new(protocol.clone());
            let unmarshaller = ChatEventsUnmarshaller::new(protocol.clone());
            for (event, expected, media) in [
                (ChatEvents::Text(TextEvent { value: #{Some}("raw string".into()) }), b"raw string".as_slice(), "text/plain"),
                (ChatEvents::Binary(BinaryEvent { value: #{Some}(#{SmithyTypes}::Blob::new(b"\x00\xff")) }), b"\x00\xff".as_slice(), "application/octet-stream"),
            ] {
                let frame = marshaller.marshall(event).unwrap();
                assert_eq!(frame.payload(), expected);
                assert_eq!(frame.headers().iter().find(|h| h.name().as_str() == ":content-type").unwrap()
                    .value().as_string().unwrap().as_str(), media);
                assert!(matches!(unmarshaller.unmarshall(&frame).unwrap(), UnmarshalledMessage::Event(_)));
            }
            let checked = ChatEvents::Checked(crate::model::CheckedEvent {
                value: #{Some}(crate::model::CheckedBody { value: #{Some}(crate::model::Kind::Valid) }),
            });
            let frame = marshaller.marshall(checked).unwrap();
            assert_eq!(frame.headers().iter().find(|h| h.name().as_str() == ":content-type").unwrap()
                .value().as_string().unwrap().as_str(), content_type);
            assert!(matches!(unmarshaller.unmarshall(&frame).unwrap(), UnmarshalledMessage::Event(ChatEvents::Checked(_))));
            let invalid = #{SmithyTypes}::event_stream::Message::new_from_parts(frame.headers().to_vec(), #{Bytes}::copy_from_slice(empty));
            assert!(unmarshaller.unmarshall(&invalid).unwrap_err().to_string().contains("constraint violation"));
            let implicit_payload = if content_type == "application/cbor" {
                let mut encoder = #{Cbor}::Encoder::new(#{Vec}::new());
                encoder.begin_map().str("value").str("valid").end();
                encoder.into_writer()
            } else {
                br##"{"value":"valid"}"##.to_vec()
            };
            let implicit = #{SmithyTypes}::event_stream::Message::new_from_parts(
                vec![
                    #{SmithyTypes}::event_stream::Header::new(":message-type", #{SmithyTypes}::event_stream::HeaderValue::String("event".into())),
                    #{SmithyTypes}::event_stream::Header::new(":event-type", #{SmithyTypes}::event_stream::HeaderValue::String("implicit".into())),
                    #{SmithyTypes}::event_stream::Header::new("from", #{SmithyTypes}::event_stream::HeaderValue::String("ann".into())),
                ],
                #{Bytes}::from(implicit_payload),
            );
            match unmarshaller.unmarshall(&implicit).unwrap() {
                UnmarshalledMessage::Event(ChatEvents::Implicit(event)) => {
                    assert_eq!(event.from.as_deref(), #{Some}("ann"));
                    assert_eq!(event.value, #{Some}(crate::model::Kind::Valid));
                }
                other => panic!("unexpected implicit event: {other:?}"),
            }
            let error_marshaller = ChatEventsErrorMarshaller::new(protocol.clone());
            let error = crate::error::ChatEventsError::StreamFailure(crate::error::StreamFailure { message: #{Some}("failure".into()) });
            let frame = error_marshaller.marshall(error).unwrap();
            if content_type == "application/cbor" {
                let mut expected = #{Cbor}::Encoder::new(#{Vec}::new());
                expected.begin_map().str("__type").str("com.aws.example.streaming##StreamFailure")
                    .str("message").str("failure").end();
                assert_eq!(frame.payload(), expected.into_writer().as_slice());
            }
            assert_eq!(frame.headers().iter().find(|h| h.name().as_str() == ":message-type").unwrap()
                .value().as_string().unwrap().as_str(), "exception");
            assert!(matches!(unmarshaller.unmarshall(&frame).unwrap(), UnmarshalledMessage::Error(crate::error::ChatEventsError::StreamFailure(_))));
            // Modeled errors also travel through the generated response and shared sender adapter.
            use #{SmithyHttpServer}::operation::StreamingOperationShape;
            let output = crate::output::ChatOutput {
                topic: #{None},
                events: #{FuturesUtil}::stream::iter([#{Err}(crate::error::ChatEventsError::StreamFailure(
                    crate::error::StreamFailure { message: #{Some}("failure".into()) }
                ))]).into(),
            };
            let response = crate::operation_shape::Chat::serialize_streaming_output(output, &protocol);
            use #{HttpBodyUtil}::BodyExt;
            let mut bytes = response.into_body().collect().await.unwrap().to_bytes();
            let mut frame = #{SmithyEventStream}::frame::read_message_from(&mut bytes).unwrap();
            if frame.headers().iter().any(|h| h.name().as_str() == ":event-type" &&
                h.value().as_string().unwrap().as_str() == "initial-response") {
                frame = #{SmithyEventStream}::frame::read_message_from(&mut bytes).unwrap();
            }
            assert!(matches!(unmarshaller.unmarshall(&frame).unwrap(), UnmarshalledMessage::Error(crate::error::ChatEventsError::StreamFailure(_))));
            assert!(bytes.is_empty());
        }
        // Direct construction must also reject header-only and primitive events without panicking.
        ##[derive(Debug)]
        struct HttpOnly;
        impl #{SmithyHttpServer}::schema::ServerProtocol for HttpOnly {
            fn build_router(&self, _: #{SmithyHttpServer}::routing::RouterBuildContext<'_>)
                -> #{Result}<#{SmithyHttpServer}::routing::SharedProtocolRouter, #{SmithyHttpServer}::routing::RouterBuildError> { unreachable!() }
            fn serialize_internal_failure(&self) -> #{SmithyHttpServer}::response::Response { unreachable!() }

            fn protocol_id(&self) -> &'static #{Schema}::ShapeId<'static> {
                static ID: #{Schema}::ShapeId<'static> = #{Schema}::shape_id!("test", "HttpOnly");
                &ID
            }
            fn deserialize_request<'a>(&'a self, _: &#{Schema}::Schema<'_>, _: &'a #{SmithyHttpServer}::schema::ServerRequest)
                -> #{Result}<#{Box}<dyn #{Schema}::serde::ShapeDeserializer + 'a>, #{SmithyHttpServer}::schema::DeserializeError> { unreachable!() }
            fn serialize_response(&self, _: &#{Schema}::Schema<'_>, _: &dyn #{Schema}::serde::SerializableStruct)
                -> #{SmithyHttpServer}::response::Response { unreachable!() }
            fn serialize_streaming_response(&self, _: &#{Schema}::Schema<'_>, _: &dyn #{Schema}::serde::SerializableStruct, _: #{SmithyHttpServer}::body::BoxBody)
                -> #{SmithyHttpServer}::response::Response { unreachable!() }
            fn serialize_error(&self, _: &dyn #{SmithyHttpServer}::schema::HttpModeledError)
                -> #{SmithyHttpServer}::response::Response { unreachable!() }
            fn serialize_rejection(&self, _: #{SmithyHttpServer}::schema::DeserializeError)
                -> #{SmithyHttpServer}::response::Response { unreachable!() }
        }
        let missing = SharedServerProtocol::new(HttpOnly);
        let marshaller = ChatEventsMarshaller::new(missing.clone());
        assert!(marshaller.marshall(ChatEvents::Bye(crate::model::Bye {})).is_err());
        assert!(marshaller.marshall(ChatEvents::Text(TextEvent { value: #{Some}("raw".into()) })).is_err());
        assert!(ChatEventsErrorMarshaller::new(missing.clone()).marshall(
            crate::error::ChatEventsError::StreamFailure(crate::error::StreamFailure { message: #{None} })).is_err());
        let unmarshaller = ChatEventsUnmarshaller::new(missing);
        assert!(unmarshaller.unmarshall(&#{SmithyTypes}::event_stream::Message::new(#{Bytes}::new())).is_err());
        """

    private val delayedInputTest =
        """
        let config = crate::service::ChatServiceConfig::builder().build();
        let service = crate::service::ChatService::builder(config)
            .chat(|input: crate::input::ChatInput| async move {
                // Preserve the Pokemon regression: input is only polled while sending output.
                let events = #{FuturesUtil}::stream::unfold(input.events, |mut receiver| async move {
                    receiver.recv().await.unwrap().map(|event| (#{Ok}(event), receiver))
                });
                #{Ok}::<_, crate::error::ChatError>(crate::output::ChatOutput { topic: #{None}, events: events.into() })
            }).build_unchecked();
        let body = #{HttpBodyUtil}::StreamBody::new(#{FuturesUtil}::stream::poll_fn(|_| {
            panic!("response creation must not poll the delayed first input event");
            ##[allow(unreachable_code)]
            std::task::Poll::Ready(#{None}::<#{Result}<#{HttpBody}::Frame<#{Bytes}>, std::convert::Infallible>>)
        }));
        let request = #{Http}::Request::builder().method("POST").uri("/chat/lobby")
            .header("content-type", "application/vnd.amazon.eventstream")
            .body(#{SmithyHttpServer}::body::boxed_sync(body)).unwrap();
        let response = #{Tower}::ServiceExt::oneshot(service, request).await.unwrap();
        assert_eq!(response.status(), #{Http}::StatusCode::OK);
        """

    private fun directionTests(rpc: Boolean): String {
        val uploadUri = if (rpc) "/service/ChatService/operation/Upload" else "/upload"
        val downloadUri = if (rpc) "/service/ChatService/operation/Download" else "/download"
        return """
        use #{SmithyHttpServer}::schema::SharedServerProtocol;
        let protocol = if $rpc {
            SharedServerProtocol::new(#{SmithyHttpServer}::protocol::rpc_v2_cbor::RpcV2CborProtocol::default())
        } else {
            SharedServerProtocol::new(#{SmithyHttpServer}::protocol::rest_json_1::RestJson1Protocol::default())
        };
        let config = crate::service::ChatServiceConfig::builder().build();
        let service = crate::service::ChatService::builder(config)
            .upload(|mut input: crate::input::UploadInput| async move {
                let mut count = 0;
                while input.events.recv().await.unwrap().is_some() { count += 1; }
                #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput { count: #{Some}(count) })
            })
            .download(|_: crate::input::DownloadInput| async move {
                #{Ok}::<_, crate::error::DownloadError>(crate::output::DownloadOutput {
                    events: #{FuturesUtil}::stream::iter([#{Ok}(crate::model::ChatEvents::Bye(crate::model::Bye {}))]).into(),
                })
            }).build_unchecked();
        $requestFrames
        // RPC must preserve a first ordinary event when there is no initial request frame.
        for (bytes, count) in [(body, 2), (#{Vec}::new(), 0)] {
            let request = #{Http}::Request::builder().method("POST").uri("$uploadUri")
                .header("smithy-protocol", "rpc-v2-cbor")
                .header("content-type", "application/vnd.amazon.eventstream")
                .body(#{SmithyHttpServer}::body::boxed_sync(#{HttpBodyUtil}::Full::new(#{Bytes}::from(bytes)))).unwrap();
            let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
            assert_eq!(response.status(), #{Http}::StatusCode::OK);
            use #{HttpBodyUtil}::BodyExt;
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            if $rpc {
                let mut decoder = #{Cbor}::Decoder::new(&bytes);
                decoder.map().unwrap();
                assert_eq!(decoder.str().unwrap(), "count");
                assert_eq!(decoder.integer().unwrap(), count);
            } else {
                assert_eq!(std::str::from_utf8(&bytes).unwrap(), format!("{{\"count\":{count}}}"));
            }
        }
        let request = #{Http}::Request::builder().method("POST").uri("$downloadUri")
            .header("smithy-protocol", "rpc-v2-cbor")
            .body(#{SmithyHttpServer}::body::boxed_sync(#{HttpBodyUtil}::Full::new(#{Bytes}::new()))).unwrap();
        let response = #{Tower}::ServiceExt::oneshot(service, request).await.unwrap();
        assert_eq!(response.status(), #{Http}::StatusCode::OK);
        use #{HttpBodyUtil}::BodyExt;
        let mut bytes = response.into_body().collect().await.unwrap().to_bytes();
        let mut kinds = #{Vec}::new();
        while !bytes.is_empty() {
            let frame = #{SmithyEventStream}::frame::read_message_from(&mut bytes).unwrap();
            kinds.push(frame.headers().iter().find(|h| h.name().as_str() == ":event-type").unwrap()
                .value().as_string().unwrap().as_str().to_owned());
        }
        assert_eq!(kinds.last().map(#{String}::as_str), #{Some}("bye"));
        assert!(kinds.len() <= 2);
        """
    }

    private val swapProtocolTest =
        """
        let protocol = #{SmithyHttpServer}::schema::SharedServerProtocol::new(
            #{SmithyHttpServer}::protocol::aws_json_11::AwsJson1_1Protocol::default());
        let selected = protocol.clone();
        let layer = #{Tower}::util::MapRequestLayer::new(move |mut request: #{Http}::Request<#{SmithyHttpServer}::body::Body>| {
            let operation = request.extensions().get::<#{SmithyHttpServer}::schema::SelectedProtocolOperation>().unwrap().operation();
            request.extensions_mut().insert(#{SmithyHttpServer}::schema::SelectedProtocolOperation::new(selected.clone(), operation));
            request
        });
        ${echoService.replace("ChatServiceConfig::builder().build()", "ChatServiceConfig::builder().http_plugin(#{SmithyHttpServer}::plugin::LayerPlugin(layer)).build()")}
        $requestFrames
        let initial = #{SmithyTypes}::event_stream::Message::new_from_parts(vec![
            #{SmithyTypes}::event_stream::Header::new(":message-type", #{SmithyTypes}::event_stream::HeaderValue::String("event".into())),
            #{SmithyTypes}::event_stream::Header::new(":event-type", #{SmithyTypes}::event_stream::HeaderValue::String("initial-request".into())),
            #{SmithyTypes}::event_stream::Header::new(":content-type", #{SmithyTypes}::event_stream::HeaderValue::String("application/json".into())),
        ], #{Bytes}::from_static(br##"{"room":"lobby","nick":"ann"}"##));
        let mut bytes = #{Vec}::new();
        #{SmithyEventStream}::frame::write_message_to(&initial, &mut bytes).unwrap();
        bytes.extend_from_slice(&body);
        let request = #{Http}::Request::builder().method("POST").uri("/service/ChatService/operation/Chat")
            .header("smithy-protocol", "rpc-v2-cbor")
            .header("content-type", "application/x-amz-json-1.1")
            .header("accept", "application/x-amz-json-1.1")
            .body(#{SmithyHttpServer}::body::boxed_sync(#{HttpBodyUtil}::Full::new(#{Bytes}::from(bytes)))).unwrap();
        let response = #{Tower}::ServiceExt::oneshot(service, request).await.unwrap();
        assert_eq!(response.status(), #{Http}::StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/x-amz-json-1.1");
        use #{HttpBodyUtil}::BodyExt;
        let mut bytes = response.into_body().collect().await.unwrap().to_bytes();
        let frame = #{SmithyEventStream}::frame::read_message_from(&mut bytes).unwrap();
        assert_eq!(frame.headers().iter().find(|h| h.name().as_str() == ":content-type").unwrap()
            .value().as_string().unwrap().as_str(), "application/json");
        assert_eq!(frame.payload(), br##"{"topic":"lobby/ann"}"##.as_slice());
        for text in ["hi", "there"] {
            let frame = #{SmithyEventStream}::frame::read_message_from(&mut bytes).unwrap();
            assert_eq!(frame.headers().iter().find(|h| h.name().as_str() == ":content-type").unwrap()
                .value().as_string().unwrap().as_str(), "application/json");
            assert_eq!(std::str::from_utf8(frame.payload()).unwrap(), format!("{{\"text\":\"{text}\"}}"));
        }
        let bye = #{SmithyEventStream}::frame::read_message_from(&mut bytes).unwrap();
        assert_eq!(bye.headers().iter().find(|h| h.name().as_str() == ":event-type").unwrap()
            .value().as_string().unwrap().as_str(), "bye");
        assert!(bytes.is_empty());
        """

    private val initialRequestTests =
        """
        use #{SmithyHttpServer}::operation::StreamingOperationShape;
        let protocol = #{SmithyHttpServer}::schema::SharedServerProtocol::new(
            #{SmithyHttpServer}::protocol::rpc_v2_cbor::RpcV2CborProtocol::default());
        $requestFrames
        // Missing initial metadata fails required-field validation, without dropping the first event.
        // Exercise receiver buffering independently, then the generated malformed/empty input paths.
        let unmarshaller = crate::event_stream_serde::ChatEventsUnmarshaller::new(protocol.clone());
        let mut receiver = #{SmithyHttp}::event_stream::Receiver::new(unmarshaller, #{SdkBody}::from(body.clone()));
        assert!(receiver.try_recv_initial(#{SmithyHttp}::event_stream::InitialMessageType::Request).await.unwrap().is_none());
        assert!(matches!(receiver.recv().await.unwrap(), #{Some}(crate::model::ChatEvents::Message(_))));
        for bytes in [body, #{Vec}::new(), b"malformed frame".to_vec()] {
            let request = #{SmithyHttpServer}::schema::ServerRequest {
                uri: #{RuntimeApi}::http::Uri::try_from("/").unwrap(), headers: Default::default(), body: #{Bytes}::new(),
            };
            let future = {
                let mut deserializer = protocol.deserialize_request(crate::input::ChatInput::SCHEMA, &request).unwrap();
                crate::operation_shape::Chat::deserialize_streaming_input(&mut *deserializer, #{SdkBody}::from(bytes), protocol.clone())
            };
            assert!(future.await.is_err());
        }
        """

    @Test
    fun `restJson1 duplex stream with the bindings in the request line and headers`() {
        val servers =
            serverIntegrationTest(
                model(rpc = false).asSmithyModel(),
                params(rpc = false),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { codegenContext, rustCrate ->
                rustCrate.testModule {
                    tokioTest("frame_payloads_and_constraints") {
                        rustTemplate(frameTests, *scope(codegenContext))
                    }
                    tokioTest("rest_response_does_not_wait_for_input") {
                        rustTemplate(delayedInputTest, *scope(codegenContext))
                    }
                    tokioTest("input_and_output_only") {
                        rustTemplate(directionTests(false), *scope(codegenContext))
                    }
                    tokioTest("streaming_blob_is_preserved") {
                        rustTemplate(
                            """
                        let config = crate::service::ChatServiceConfig::builder().build();
                        let service = crate::service::ChatService::builder(config)
                            .blob_echo(|input: crate::input::BlobEchoInput| async move {
                                #{Ok}::<_, crate::error::BlobEchoError>(crate::output::BlobEchoOutput { data: input.data })
                            }).build_unchecked();
                        let request = #{Http}::Request::builder().method("POST").uri("/blob")
                            .body(#{SmithyHttpServer}::body::boxed_sync(#{HttpBodyUtil}::Full::new(#{Bytes}::from_static(b"raw blob")))).unwrap();
                        let response = #{Tower}::ServiceExt::oneshot(service, request).await.unwrap();
                        assert_eq!(response.status(), #{Http}::StatusCode::OK);
                        use #{HttpBodyUtil}::BodyExt;
                        assert_eq!(response.into_body().collect().await.unwrap().to_bytes(), b"raw blob".as_slice());
                        """,
                            *scope(codegenContext),
                        )
                    }
                    tokioTest("chat_over_rest_json_1") {
                        rustTemplate(
                            """
                        let protocol = #{SmithyHttpServer}::schema::SharedServerProtocol::new(#{Protocol}::default());
                        $echoService
                        $requestFrames
                        let request = #{Http}::Request::builder()
                            .method("POST")
                            .uri("/chat/lobby")
                            .header("x-nick", "ann")
                            .header("content-type", "application/vnd.amazon.eventstream")
                            .header("accept", "application/vnd.amazon.eventstream")
                            .body(#{SmithyHttpServer}::body::boxed_sync(#{HttpBodyUtil}::Full::new(#{Bytes}::from(body))))
                            .unwrap();
                        let response = #{Tower}::ServiceExt::oneshot(service, request).await.expect("infallible");
                        $responseFrames
                        assert_eq!(parts.headers.get("x-topic").unwrap(), "lobby/ann");
                        $echoedEvents
                        """,
                            *scope(codegenContext),
                            "Protocol" to ServerRuntimeType.protocol("RestJson1Protocol", "rest_json_1", codegenContext.runtimeConfig),
                        )
                    }
                }
            }
        servers.forEach { check(!it.path.resolve("src/protocol_serde").toFile().exists()) }
    }

    @Test
    fun `rpcv2Cbor duplex stream with initial-request and initial-response frames`() = rpcTest(true)

    @Test
    fun `rpcv2Cbor can disable initial-response frames`() = rpcTest(false)

    private fun rpcTest(sendInitial: Boolean) {
        val servers =
            serverIntegrationTest(
                model(rpc = true).asSmithyModel(),
                params(rpc = true, sendInitial = sendInitial),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { codegenContext, rustCrate ->
                rustCrate.testModule {
                    tokioTest("frame_payloads_and_constraints") {
                        rustTemplate(frameTests, *scope(codegenContext))
                    }
                    tokioTest("initial_request_edge_cases") {
                        rustTemplate(initialRequestTests, *scope(codegenContext))
                    }
                    tokioTest("input_and_output_only") {
                        rustTemplate(directionTests(true), *scope(codegenContext))
                    }
                    if (sendInitial) {
                        tokioTest("selected_alternative_protocol_controls_all_frames") {
                            rustTemplate(swapProtocolTest, *scope(codegenContext))
                        }
                    }
                    tokioTest("chat_over_rpcv2_cbor") {
                        rustTemplate(
                            """
                        let protocol = #{SmithyHttpServer}::schema::SharedServerProtocol::new(#{Protocol}::default());
                        $echoService
                        // The non-stream input members travel in the `initial-request` frame, encoded by the
                        // protocol's payload codec.
                        let mut body = #{Vec}::new();
                        {
                            use #{SmithyTypes}::event_stream::{Header, HeaderValue, Message};
                            let payload = {
                                let mut encoder = #{Cbor}::Encoder::new(#{Vec}::new());
                                encoder.begin_map().str("room").str("lobby").str("nick").str("ann").end();
                                encoder.into_writer()
                            };
                            let headers = vec![
                                Header::new(":message-type", HeaderValue::String("event".into())),
                                Header::new(":event-type", HeaderValue::String("initial-request".into())),
                                Header::new(":content-type", HeaderValue::String("application/cbor".into())),
                            ];
                            let message = Message::new_from_parts(headers, #{Bytes}::from(payload));
                            #{SmithyEventStream}::frame::write_message_to(&message, &mut body).expect("writes");
                        }
                        let events = {
                            $requestFrames
                            body
                        };
                        body.extend_from_slice(&events);
                        let request = #{Http}::Request::builder()
                            .method("POST")
                            .uri("/service/ChatService/operation/Chat")
                            .header("smithy-protocol", "rpc-v2-cbor")
                            .header("content-type", "application/vnd.amazon.eventstream")
                            .header("accept", "application/vnd.amazon.eventstream")
                            .body(#{SmithyHttpServer}::body::boxed_sync(#{HttpBodyUtil}::Full::new(#{Bytes}::from(body))))
                            .unwrap();
                        let response = #{Tower}::ServiceExt::oneshot(service, request).await.expect("infallible");
                        $responseFrames
                        assert_eq!(parts.headers.get("smithy-protocol").unwrap(), "rpc-v2-cbor");
                        // The `initial-response` frame carries the non-stream output members through the
                        // protocol's payload codec.
                        assert_eq!(initial, $sendInitial);
                        if $sendInitial {
                        let initial = receiver
                            .try_recv_initial(#{SmithyHttp}::event_stream::InitialMessageType::Response)
                            .await
                            .expect("a frame")
                            .expect("an initial-response frame");
                        let content_type = initial
                            .headers()
                            .iter()
                            .find(|header| header.name().as_str() == ":content-type")
                            .map(|header| header.value().as_string().unwrap().as_str().to_owned());
                        assert_eq!(content_type.as_deref(), #{Some}("application/cbor"));
                        let mut decoder = #{Cbor}::Decoder::new(initial.payload());
                        decoder.map().unwrap();
                        assert_eq!(decoder.str().unwrap(), "topic");
                        assert_eq!(decoder.str().unwrap(), "lobby/ann");
                        }
                        $echoedEvents
                        """,
                            *scope(codegenContext),
                            "Cbor" to CargoDependency.smithyCbor(codegenContext.runtimeConfig).toType(),
                            "Protocol" to ServerRuntimeType.protocol("RpcV2CborProtocol", "rpc_v2_cbor", codegenContext.runtimeConfig),
                        )
                    }
                }
            }
        servers.forEach { check(!it.path.resolve("src/protocol_serde").toFile().exists()) }
    }
}
