/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class EventStreamMessageTimeoutTest {
    private val model =
        """
        ${'$'}version: "2.0"
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service EventService {
            operations: [PublishEvents, Echo]
        }

        @http(uri: "/publish", method: "POST")
        operation PublishEvents {
            input := {
                @httpPayload
                @required
                events: Events
            }
            output := {}
        }

        @http(uri: "/echo", method: "POST")
        operation Echo {
            input := {
                @required
                message: String
            }
            output := {
                @required
                message: String
            }
        }

        @streaming
        union Events {
            event: Event
        }

        structure Event {
            data: String
        }
        """.asSmithyModel()

    private val streamingBlobModel =
        """
        ${'$'}version: "2.0"
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service StreamingService {
            operations: [StreamingUpload]
        }

        @http(uri: "/upload", method: "POST")
        operation StreamingUpload {
            input := {
                @httpPayload
                @required
                data: StreamingBlob
            }
        }

        @streaming
        blob StreamingBlob
        """.asSmithyModel()

    @Test
    fun `event stream message timeout is disabled by default`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#EventService"),
                null,
            )

        check(config.eventStreamMessageTimeoutMillisFor(ShapeId.from("test#PublishEvents")) == null)
        // Event stream operations still get no whole-body read deadline
        check(config.timeoutMillisFor(ShapeId.from("test#PublishEvents")) == null)
        // The message timeout never applies to non-event-stream operations
        check(config.eventStreamMessageTimeoutMillisFor(ShapeId.from("test#Echo")) == null)
    }

    @Test
    fun `event stream message timeout default can be configured`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#EventService"),
                objectNode(
                    """
                    {
                        "protocols": {
                            "global": {
                                "requestBodyReadTimeouts": {
                                    "defaultEventStreamMessage": "30s"
                                }
                            }
                        }
                    }
                    """,
                ),
            )

        check(config.eventStreamMessageTimeoutMillisFor(ShapeId.from("test#PublishEvents")) == 30_000L)
        check(config.eventStreamMessageTimeoutMillisFor(ShapeId.from("test#Echo")) == null)
        // The event stream message timeout doesn't enable the whole-body deadline for streaming operations
        check(config.timeoutMillisFor(ShapeId.from("test#PublishEvents")) == null)
    }

    @Test
    fun `per operation timeout on event stream operation configures the message timeout`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#EventService"),
                objectNode(
                    """
                    {
                        "protocols": {
                            "global": {
                                "requestBodyReadTimeouts": {
                                    "defaultEventStreamMessage": "30s",
                                    "perOperation": {
                                        "test#PublishEvents": "5s"
                                    }
                                }
                            }
                        }
                    }
                    """,
                ),
            )

        check(config.eventStreamMessageTimeoutMillisFor(ShapeId.from("test#PublishEvents")) == 5_000L)
    }

    @Test
    fun `per operation zero disables the event stream message timeout`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#EventService"),
                objectNode(
                    """
                    {
                        "protocols": {
                            "global": {
                                "requestBodyReadTimeouts": {
                                    "defaultEventStreamMessage": "30s",
                                    "perOperation": {
                                        "test#PublishEvents": 0
                                    }
                                }
                            }
                        }
                    }
                    """,
                ),
            )

        check(config.eventStreamMessageTimeoutMillisFor(ShapeId.from("test#PublishEvents")) == null)
    }

    @Test
    fun `per operation timeout on streaming blob operation is still rejected`() {
        val error =
            assertThrows<CodegenException> {
                RequestBodyReadTimeouts.fromCustomizationConfig(
                    streamingBlobModel,
                    ShapeId.from("test#StreamingService"),
                    objectNode(
                        """
                        {
                            "protocols": {
                                "global": {
                                    "requestBodyReadTimeouts": {
                                        "perOperation": {
                                            "test#StreamingUpload": "30s"
                                        }
                                    }
                                }
                            }
                        }
                        """,
                    ),
                )
            }

        check(error.message?.contains("are not supported for streaming inputs") == true)
    }

    @Test
    fun `service compiles with event stream message timeout and generated code applies it`() {
        val generatedServers =
            serverIntegrationTest(
                model,
                IntegrationTestParams(additionalSettings = messageTimeoutSettings()),
            )
        generatedServers.forEach { generatedServer ->
            val protocolSerde = protocolSerdeContents(generatedServer.path)
            check(protocolSerde.contains("wrap_with_event_stream_timeouts(")) {
                "expected the generated deserializer to apply the event stream message timeout"
            }
            check(protocolSerde.contains("::std::time::Duration::from_millis(45000u64)")) {
                "expected the configured 45s timeout in the generated deserializer"
            }
        }
    }

    @Test
    fun `handlers can detect the message timeout and keep the stream alive`() {
        serverIntegrationTest(
            model,
            IntegrationTestParams(additionalSettings = messageTimeoutSettings()),
        ) { codegenContext, rustCrate ->
            rustCrate.testModule {
                tokioTest("message_timeout_is_detectable_and_stream_survives") {
                    rustTemplate(
                        """
                        use crate::event_stream_message_timeout::MessageTimeoutError;
                        use std::pin::Pin;
                        use std::task::{Context, Poll};

                        struct PartialThenHang {
                            sent: bool,
                        }

                        impl http_body_1x::Body for PartialThenHang {
                            type Data = bytes::Bytes;
                            type Error = std::convert::Infallible;

                            fn poll_frame(
                                mut self: Pin<&mut Self>,
                                _cx: &mut Context<'_>,
                            ) -> Poll<Option<Result<http_body_1x::Frame<bytes::Bytes>, Self::Error>>> {
                                if !self.sent {
                                    self.sent = true;
                                    // The first 6 bytes of a frame claiming 32 total bytes; the
                                    // rest never arrives. Only the timeout can wake the task.
                                    Poll::Ready(Some(Ok(http_body_1x::Frame::data(
                                        bytes::Bytes::from_static(&[0, 0, 0, 32, 0, 0]),
                                    ))))
                                } else {
                                    Poll::Pending
                                }
                            }
                        }

                        let body = crate::event_stream_message_timeout::wrap_with_message_timeout(
                            aws_smithy_types::body::SdkBody::from_body_1_x(PartialThenHang { sent: false }),
                            std::time::Duration::from_millis(50),
                        );
                        let mut stream = #{Receiver}::new(
                            crate::event_stream_serde::EventsUnmarshaller::new(),
                            body,
                        );
                        let err = stream
                            .recv()
                            .await
                            .expect_err("the stalled message must time out");
                        let timeout = MessageTimeoutError::find_in_chain(&err)
                            .expect("handlers can detect the timeout in the error source chain");
                        assert_eq!(std::time::Duration::from_millis(50), timeout.timeout());
                        // The error is resumable: a handler that chooses to keep waiting calls
                        // recv() again, which grants one more timeout window and times out again
                        // here because the sender never completes the message.
                        let err = stream
                            .recv()
                            .await
                            .expect_err("continuing grants one more window, which also expires");
                        assert!(MessageTimeoutError::find_in_chain(&err).is_some());
                        """,
                        "Receiver" to RuntimeType.eventStreamReceiver(codegenContext.runtimeConfig),
                    )
                }
            }
        }
    }

    @Test
    fun `generated code applies a one hour idle timeout by default`() {
        val generatedServers =
            serverIntegrationTest(model) { codegenContext, rustCrate ->
                rustCrate.testModule {
                    tokioTest("default_idle_deadline_bounds_first_event_wait_and_gaps_after_events") {
                        rustTemplate(
                            """
                        use #{SmithyHttpServer}::request::FromRequest;
                        use crate::event_stream_message_timeout::MessageTimeoutError;
                        struct IdleBody { first: #{Option}<#{Bytes}> }
                        impl #{Body1x}::Body for IdleBody {
                            type Data = #{Bytes};
                            type Error = std::convert::Infallible;
                            fn poll_frame(
                                mut self: std::pin::Pin<&mut Self>,
                                _cx: &mut std::task::Context<'_>,
                            ) -> std::task::Poll<#{Option}<#{Result}<#{Body1x}::Frame<#{Bytes}>, Self::Error>>> {
                                if let #{Some}(data) = self.first.take() {
                                    std::task::Poll::Ready(#{Some}(#{Ok}(#{Body1x}::Frame::data(data))))
                                } else {
                                    std::task::Poll::Pending
                                }
                            }
                        }
                        tokio::time::pause();
                        use #{SmithyTypes}::event_stream::{Header, HeaderValue, Message};
                        let message = Message::new(#{Bytes}::from_static(b"{\"data\":\"hello\"}"))
                            .add_header(Header::new(":message-type", HeaderValue::String("event".into())))
                            .add_header(Header::new(":event-type", HeaderValue::String("event".into())))
                            .add_header(Header::new(":content-type", HeaderValue::String("application/json".into())));
                        let mut frame = #{Vec}::new();
                        #{EventStream}::frame::write_message_to(&message, &mut frame).unwrap();
                        for first in [#{None}, #{Some}(#{Bytes}::from(frame))] {
                            let has_event = first.is_some();
                            let request = #{Http}::Request::builder()
                                .uri("/publish").method("POST")
                                .header("Content-Type", "application/vnd.amazon.eventstream")
                                .body(#{SdkBody}::from_body_1_x(IdleBody { first })).unwrap();
                            let mut input = crate::input::PublishEventsInput::from_request(request).await.unwrap();
                            if has_event {
                                let event = input.events.recv().await.unwrap().unwrap();
                                assert_eq!(event.as_event().unwrap().data(), #{Some}("hello"));
                            }
                            let start = tokio::time::Instant::now();
                            let error = input.events.recv().await.unwrap_err();
                            let timeout = MessageTimeoutError::find_in_chain(&error).unwrap();
                            assert!(timeout.is_idle());
                            assert_eq!(timeout.timeout(), std::time::Duration::from_secs(3600));
                            let elapsed = start.elapsed();
                            assert!(elapsed >= std::time::Duration::from_secs(3600));
                            assert!(elapsed <= std::time::Duration::from_millis(3_600_001));
                        }
                        """,
                            *RuntimeType.preludeScope,
                            "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                            "Body1x" to CargoDependency.HttpBody1x.toType(),
                            "Bytes" to RuntimeType.Bytes,
                            "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                            "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                            "SmithyTypes" to RuntimeType.smithyTypes(codegenContext.runtimeConfig),
                            "EventStream" to RuntimeType.smithyEventStream(codegenContext.runtimeConfig),
                        )
                    }
                }
            }
        generatedServers.forEach { generatedServer ->
            val protocolSerde = protocolSerdeContents(generatedServer.path)
            check(protocolSerde.contains("wrap_with_event_stream_timeouts(")) {
                "the event stream idle timeout must be on by default"
            }
            check(protocolSerde.contains("from_millis(3600000u64)"))
        }
    }

    @Test
    fun `generated code omits the timeout wrapper when all deadlines are disabled`() {
        val settings =
            objectNode(
                """{"customizationConfig": {"protocols": {"global": {"eventStreamTimeouts": {"idle": 0}}}}}""",
            )
        val generatedServers = serverIntegrationTest(model, IntegrationTestParams(additionalSettings = settings))
        generatedServers.forEach { generatedServer ->
            check(!protocolSerdeContents(generatedServer.path).contains("wrap_with_event_stream_timeouts("))
        }
    }

    private fun protocolSerdeContents(crateDir: java.nio.file.Path): String =
        crateDir.resolve("src/protocol_serde").toFile().walkTopDown()
            .filter { it.isFile && it.extension == "rs" }
            .joinToString("\n") { it.readText() }

    private fun messageTimeoutSettings(): ObjectNode =
        objectNode(
            """
            {
                "customizationConfig": {
                    "protocols": {
                        "global": {
                            "eventStreamTimeouts": {
                                "messageCompletion": "45s"
                            }
                        }
                    }
                }
            }
            """,
        )

    private fun objectNode(json: String): ObjectNode = Node.parse(json).expectObjectNode()
}
