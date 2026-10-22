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
import kotlin.time.Duration
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Duration.Companion.seconds

internal class EventStreamTimeoutsTest {
    private fun model(protocol: String = "aws.protocols#restJson1") =
        """
        ${'$'}version: "2.0"
        namespace test
        use $protocol
        @${protocol.substringAfter("#")}
        service EventService { operations: [PublishEvents, Echo] }
        @http(uri: "/publish", method: "POST")
        operation PublishEvents {
            input := {
                ${if (protocol == "aws.protocols#restJson1") "@httpPayload" else ""}
                @required
                events: Events
                ${if (protocol == "smithy.protocols#rpcv2Cbor") "metadata: String" else ""}
            }
            output := {}
        }
        @http(uri: "/echo", method: "POST")
        operation Echo {
            input := { @required message: String }
            output := { @required message: String }
        }
        @streaming
        union Events { event: Event }
        structure Event { data: String }
        """.asSmithyModel()

    private val publish = ShapeId.from("test#PublishEvents")
    private val echo = ShapeId.from("test#Echo")

    private fun settings(
        global: String,
        bodyTimeouts: String? = null,
    ): ServerRustSettings =
        ServerRustSettings.from(
            model(),
            Node.parse(
                """
                {
                    "service": "test#EventService",
                    "module": "test-service",
                    "moduleVersion": "0.0.1",
                    "moduleAuthors": ["test"],
                    "customizationConfig": {
                        "protocols": {
                            "common": {
                                "eventStreamTimeouts": $global
                                ${if (bodyTimeouts != null) ""","requestBodyReadTimeouts": $bodyTimeouts""" else ""}
                            }
                        }
                    }
                }
                """,
            ).expectObjectNode(),
        )

    @Test
    fun `all event stream deadlines are independent and disabled by default`() {
        val defaults = settings("{}").eventStreamTimeouts
        check(defaults.initialMessageTimeoutFor(publish) == null)
        check(defaults.messageCompletionTimeoutFor(publish) == null)
        check(defaults.idleTimeoutFor(publish) == null)
        check(defaults.initialMessageTimeoutFor(echo) == null)
        check(defaults.messageCompletionTimeoutFor(echo) == null)
        check(defaults.idleTimeoutFor(echo) == null)
        val initialOnly = settings("""{"initialMessage": "10s"}""").eventStreamTimeouts
        check(initialOnly.initialMessageTimeoutFor(publish) == 10.seconds)
        check(initialOnly.messageCompletionTimeoutFor(publish) == null)
        check(initialOnly.idleTimeoutFor(publish) == null)
        val completionOnly = settings("""{"messageCompletion": "30s"}""").eventStreamTimeouts
        check(completionOnly.initialMessageTimeoutFor(publish) == null)
        check(completionOnly.messageCompletionTimeoutFor(publish) == 30.seconds)
        check(completionOnly.idleTimeoutFor(publish) == null)
        check(initialOnly.initialMessageTimeoutFor(echo) == null)
        check(completionOnly.messageCompletionTimeoutFor(echo) == null)
    }

    @Test
    fun `operation overrides change or disable either timeout independently`() {
        val timeouts =
            settings(
                """
                {
                    "initialMessage": "10s",
                    "messageCompletion": "30s",
                    "perOperation": {
                        "test#PublishEvents": {"initialMessage": 0, "messageCompletion": "5s"}
                    }
                }
                """,
            ).eventStreamTimeouts
        check(timeouts.initialMessageTimeoutFor(publish) == null)
        check(timeouts.messageCompletionTimeoutFor(publish) == 5.seconds)
        val fallback =
            settings(
                """{"initialMessage": "10s", "messageCompletion": "30s", "perOperation": {"test#PublishEvents": {"messageCompletion": 0}}}""",
            ).eventStreamTimeouts
        check(fallback.initialMessageTimeoutFor(publish) == 10.seconds)
        check(fallback.messageCompletionTimeoutFor(publish) == null)
    }

    @Test
    fun `request body read timeouts never configure event stream deadlines`() {
        // `requestBodyReadTimeouts` is for non-streaming bodies only: event stream deadlines
        // come exclusively from `eventStreamTimeouts`, and an entry for an event stream
        // operation in `requestBodyReadTimeouts.perOperation` fails generation.
        val bodyTimeouts = """{"defaultNonPayload": "45s"}"""
        val disabled = settings("{}", bodyTimeouts).eventStreamTimeouts
        check(disabled.initialMessageTimeoutFor(publish) == null)
        check(disabled.messageCompletionTimeoutFor(publish) == null)
        val serviceSettings = settings("{}", bodyTimeouts)
        check(serviceSettings.requestBodyReadTimeouts.timeoutFor(publish) == null)
        val absent = EventStreamTimeouts.fromCustomizationConfig(null, serviceSettings.requestBodyReadTimeouts)
        check(absent.initialMessageTimeoutFor(publish) == null)
        check(absent.messageCompletionTimeoutFor(publish) == null)
        check(absent.idleTimeoutFor(publish) == null)
        val error =
            assertThrows<CodegenException> {
                settings("{}", """{"perOperation": {"test#PublishEvents": "5s"}}""")
            }
        check(error.message?.contains("are not supported for streaming inputs") == true)
        check(error.message?.contains("eventStreamTimeouts") == true)
    }

    @Test
    fun `idle timeout can be configured overridden and disabled independently`() {
        val global = settings("""{"idle": "5m"}""").eventStreamTimeouts
        check(global.idleTimeoutFor(publish) == 5.minutes)
        check(global.initialMessageTimeoutFor(publish) == null)
        check(global.messageCompletionTimeoutFor(publish) == null)
        check(settings("""{"idle": 0}""").eventStreamTimeouts.idleTimeoutFor(publish) == null)
        val disabled =
            settings(
                """{"idle": "5m", "perOperation": {"test#PublishEvents": {"idle": 0}}}""",
            ).eventStreamTimeouts
        check(disabled.idleTimeoutFor(publish) == null)
        val override =
            settings(
                """{"perOperation": {"test#PublishEvents": {"idle": "10s"}}}""",
            ).eventStreamTimeouts
        check(override.idleTimeout == Duration.ZERO)
        check(override.idleTimeoutFor(publish) == 10.seconds)
        val inherited =
            settings(
                """{"idle": "5m", "perOperation": {"test#PublishEvents": {"messageCompletion": "10s"}}}""",
            ).eventStreamTimeouts
        check(inherited.idleTimeoutFor(publish) == 5.minutes)
    }

    @Test
    fun `malformed event timeout values and operation overrides fail generation`() {
        for (json in listOf(
            "null",
            "[]",
            """{"initialMessage": -1}""",
            """{"messageCompletion": 30}""",
            """{"initialMessage": "0.5s"}""",
            """{"messageCompletion": "9223372036854775807h"}""",
            """{"initialMessage": null}""",
            """{"idle": -1}""",
            """{"idle": 30}""",
            """{"idle": "0.5s"}""",
            """{"idle": "9223372036854775807h"}""",
            """{"idle": null}""",
            """{"perOperation": {"test#PublishEvents": {"idle": "5d"}}}""",
            """{"perOperation": null}""",
            """{"perOperation": {"test#Echo": {"initialMessage": "10s"}}}""",
            """{"perOperation": {"test#NotAttached": {"messageCompletion": "10s"}}}""",
            """{"perOperation": {"not a shape ID": {}}}""",
            """{"perOperation": {"test#PublishEvents": "10s"}}""",
        )) {
            val error = assertThrows<CodegenException> { settings(json) }
            check(error.message!!.contains("customizationConfig.protocols.common.eventStreamTimeouts"))
        }
    }

    @Test
    fun `rpc v2 cbor initial and event deadlines work in generated code`() {
        checkRpcDeadlines("smithy.protocols#rpcv2Cbor")
    }

    @Test
    fun `generated REST receiver applies detectable and resumable message timeouts`() {
        val generatedServers =
            serverIntegrationTest(
                model(),
                IntegrationTestParams(
                    additionalSettings =
                        objectNode(
                            """{"customizationConfig": {"protocols": {"common": {"eventStreamTimeouts": {"messageCompletion": "45s"}}}}}""",
                        ),
                ),
            ) { codegenContext, rustCrate ->
                rustCrate.testModule {
                    tokioTest("generated_message_timeout_is_detectable_and_stream_survives") {
                        rustTemplate(
                            """
                            use crate::event_stream_message_timeout::MessageTimeoutError;
                            use #{SmithyHttpServer}::request::FromRequest;
                            use std::pin::Pin;
                            use std::task::{Context, Poll};

                            struct PartialThenHang { sent: bool }
                            impl #{Body1x}::Body for PartialThenHang {
                                type Data = #{Bytes};
                                type Error = std::convert::Infallible;
                                fn poll_frame(
                                    mut self: Pin<&mut Self>,
                                    _cx: &mut Context<'_>,
                                ) -> Poll<#{Option}<#{Result}<#{Body1x}::Frame<#{Bytes}>, Self::Error>>> {
                                    if !self.sent {
                                        self.sent = true;
                                        // Only the first six bytes of a 32-byte frame arrive.
                                        Poll::Ready(#{Some}(#{Ok}(#{Body1x}::Frame::data(
                                            #{Bytes}::from_static(&[0, 0, 0, 32, 0, 0]),
                                        ))))
                                    } else {
                                        Poll::Pending
                                    }
                                }
                            }

                            tokio::time::pause();
                            let request = #{Http}::Request::builder()
                                .uri("/publish").method("POST")
                                .header("Content-Type", "application/vnd.amazon.eventstream")
                                .body(#{SdkBody}::from_body_1_x(PartialThenHang { sent: false }))
                                .unwrap();
                            let mut input = crate::input::PublishEventsInput::from_request(request).await.unwrap();
                            // The first error is detectable; retrying recv grants another window.
                            for _ in 0..2 {
                                let err = input.events.recv().await
                                    .expect_err("the stalled message must time out");
                                let timeout = MessageTimeoutError::find_in_chain(&err)
                                    .expect("handlers can detect the timeout in the error source chain");
                                assert_eq!(std::time::Duration::from_secs(45), timeout.timeout());
                                assert!(!timeout.is_idle());
                                assert!(!timeout.is_initial_message());
                            }
                            """,
                            *RuntimeType.preludeScope,
                            "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                            "Body1x" to CargoDependency.HttpBody1x.toType(),
                            "Bytes" to RuntimeType.Bytes,
                            "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                            "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                        )
                    }
                }
            }
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
    fun `generated code leaves first event wait and gaps unbounded by default`() {
        val generatedServers =
            serverIntegrationTest(model()) { codegenContext, rustCrate ->
                rustCrate.testModule {
                    tokioTest("default_idle_deadline_is_disabled_before_first_event_and_between_events") {
                        rustTemplate(
                            """
                            use #{SmithyHttpServer}::request::FromRequest;
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
                                assert!(
                                    tokio::time::timeout(
                                        std::time::Duration::from_secs(7200),
                                        input.events.recv(),
                                    ).await.is_err(),
                                    "recv must remain pending beyond the former one-hour idle default",
                                );
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
            check(!protocolSerdeContents(generatedServer.path).contains("wrap_with_event_stream_timeouts(")) {
                "event stream deadlines must be disabled by default"
            }
        }
    }

    @Test
    fun `generated code omits the timeout wrapper when all deadlines are disabled`() {
        val settings =
            objectNode(
                """{"customizationConfig": {"protocols": {"common": {"eventStreamTimeouts": {"initialMessage": 0, "messageCompletion": 0, "idle": 0}}}}}""",
            )
        val generatedServers = serverIntegrationTest(model(), IntegrationTestParams(additionalSettings = settings))
        generatedServers.forEach { generatedServer ->
            check(!protocolSerdeContents(generatedServer.path).contains("wrap_with_event_stream_timeouts("))
        }
    }

    private fun protocolSerdeContents(crateDir: java.nio.file.Path): String =
        crateDir.resolve("src/protocol_serde").toFile().walkTopDown()
            .filter { it.isFile && it.extension == "rs" }
            .joinToString("\n") { it.readText() }

    private fun objectNode(json: String): ObjectNode = Node.parse(json).expectObjectNode()

    private fun checkRpcDeadlines(protocol: String) {
        serverIntegrationTest(
            model(protocol),
            IntegrationTestParams(
                additionalSettings =
                    Node.parse(
                        """
                        {
                            "customizationConfig": {
                                "protocols": {
                                    "common": {
                                        "eventStreamTimeouts": {
                                            "initialMessage": "500ms",
                                            "messageCompletion": "20ms",
                                            "idle": "5s",
                                            "perOperation": {
                                                "test#PublishEvents": {"idle": "2s"}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        """,
                    ).expectObjectNode(),
            ),
        ) { codegenContext, rustCrate ->
            val cbor = protocol == "smithy.protocols#rpcv2Cbor"
            val uri = if (cbor) "/service/EventService/operation/PublishEvents" else "/"
            val headers =
                if (cbor) {
                    """.header("smithy-protocol", "rpc-v2-cbor")"""
                } else {
                    """.header("x-amz-target", "EventService.PublishEvents")"""
                }
            val scope =
                arrayOf(
                    *RuntimeType.preludeScope,
                    "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                    "Body1x" to CargoDependency.HttpBody1x.toType(),
                    "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                    "Bytes" to RuntimeType.Bytes,
                    "SmithyTypes" to RuntimeType.smithyTypes(codegenContext.runtimeConfig),
                    "EventStream" to RuntimeType.smithyEventStream(codegenContext.runtimeConfig),
                    "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                    "Tower" to RuntimeType.Tower,
                )
            rustCrate.testModule {
                rustTemplate(
                    """
                    use std::collections::VecDeque;
                    use std::future::Future;
                    use std::pin::Pin;
                    use std::task::{Context, Poll};
                    use std::time::Duration;
                    fn assert_elapsed(start: tokio::time::Instant, minimum_ms: u64, maximum_ms: u64) {
                        // Pausing an already-started runtime can leave sub-millisecond
                        // offsets; each Sleep wakeup may round up by one timer tick.
                        let elapsed = start.elapsed();
                        assert!(
                            elapsed >= Duration::from_millis(minimum_ms)
                                && elapsed <= Duration::from_millis(maximum_ms),
                            "unexpected elapsed time: {elapsed:?}, expected {minimum_ms}..={maximum_ms}ms",
                        );
                    }
                    struct TestBody {
                        steps: VecDeque<(Duration, #{Bytes})>,
                        ready: #{Option}<#{Bytes}>,
                        sleep: #{Option}<Pin<#{Box}<tokio::time::Sleep>>>,
                        hang: bool,
                    }
                    impl #{Body1x}::Body for TestBody {
                        type Data = #{Bytes};
                        type Error = std::convert::Infallible;
                        fn poll_frame(
                            mut self: Pin<&mut Self>,
                            cx: &mut Context<'_>,
                        ) -> Poll<#{Option}<#{Result}<#{Body1x}::Frame<#{Bytes}>, Self::Error>>> {
                            loop {
                                if let #{Some}(sleep) = self.sleep.as_mut() {
                                    if sleep.as_mut().poll(cx).is_pending() {
                                        return Poll::Pending;
                                    }
                                    self.sleep = #{None};
                                }
                                if let #{Some}(bytes) = self.ready.take() {
                                    return Poll::Ready(#{Some}(#{Ok}(#{Body1x}::Frame::data(bytes))));
                                }
                                if let #{Some}((delay, bytes)) = self.steps.pop_front() {
                                    self.ready = #{Some}(bytes);
                                    if !delay.is_zero() {
                                        self.sleep = #{Some}(#{Box}::pin(tokio::time::sleep(delay)));
                                    }
                                } else {
                                    return if self.hang { Poll::Pending } else { Poll::Ready(#{None}) };
                                }
                            }
                        }
                    }
                    fn request(steps: #{Vec}<(Duration, #{Bytes})>, hang: bool) -> #{Http}::Request<#{SdkBody}> {
                        #{Http}::Request::builder()
                            .uri("$uri").method("POST")
                            .header("Content-Type", "application/vnd.amazon.eventstream")
                            $headers
                            .body(#{SdkBody}::from_body_1_x(TestBody {
                                steps: steps.into(), ready: #{None}, sleep: #{None}, hang,
                            })).unwrap()
                    }
                    fn initial_frame() -> #{Bytes} {
                        use #{SmithyTypes}::event_stream::{Header, HeaderValue, Message};
                        let payload = #{Bytes}::from_static(${if (cbor) "&[0xa0]" else """b"{}""""});
                        let message = Message::new(payload)
                            .add_header(Header::new(":message-type", HeaderValue::String("event".into())))
                            .add_header(Header::new(":event-type", HeaderValue::String("initial-request".into())));
                        let mut bytes = #{Vec}::new();
                        #{EventStream}::frame::write_message_to(&message, &mut bytes).unwrap();
                        bytes.into()
                    }
                    async fn must_not_run(
                        _input: crate::input::PublishEventsInput,
                    ) -> #{Result}<crate::output::PublishEventsOutput, crate::error::PublishEventsError> {
                        panic!("initial-message timeout must reject before running the handler");
                    }
                    """,
                    *scope,
                )
                tokioTest("initial_deadline_rejects_no_bytes_and_partial_frame_before_handler") {
                    rustTemplate(
                        """
                        tokio::time::pause();
                        for steps in [
                            #{Vec}::new(),
                            vec![(Duration::from_millis(490), initial_frame().slice(..6))],
                        ] {
                            let config = crate::EventServiceConfig::builder().build();
                            let service = crate::EventService::builder(config)
                                .publish_events(must_not_run).build_unchecked();
                            let start = tokio::time::Instant::now();
                            let response = #{Tower}::ServiceExt::oneshot(service, request(steps, true)).await.unwrap();
                            assert!(response.status().is_client_error(), "{:?}", response.status());
                            assert_elapsed(start, 500, 501);
                        }
                        """,
                        *scope,
                    )
                }
                tokioTest("initial_read_uses_its_own_window_then_event_deadline_starts") {
                    rustTemplate(
                        """
                        use #{SmithyHttpServer}::request::FromRequest;
                        use crate::event_stream_message_timeout::MessageTimeoutError;
                        tokio::time::pause();
                        let initial = initial_frame();
                        let start = tokio::time::Instant::now();
                        let request = request(vec![
                            (Duration::ZERO, initial.slice(..6)),
                            (Duration::from_millis(200), initial.slice(6..)),
                            (Duration::from_millis(1000), #{Bytes}::from_static(&[0, 0, 0, 32, 0, 0])),
                        ], true);
                        let mut input = crate::input::PublishEventsInput::from_request(request).await.unwrap();
                        assert_elapsed(start, 200, 201);
                        let error = input.events.recv().await.unwrap_err();
                        let timeout = MessageTimeoutError::find_in_chain(&error).unwrap();
                        assert!(!timeout.is_initial_message());
                        assert_eq!(timeout.timeout(), Duration::from_millis(20));
                        assert_elapsed(start, 1220, 1223);
                        """,
                        *scope,
                    )
                }
                tokioTest("complete_initial_read_releases_deadline_before_handler") {
                    rustTemplate(
                        """
                        tokio::time::pause();
                        let config = crate::EventServiceConfig::builder().build();
                        let service = crate::EventService::builder(config)
                            .publish_events(|_input: crate::input::PublishEventsInput| async {
                                tokio::time::sleep(Duration::from_millis(1000)).await;
                                #{Ok}::<_, crate::error::PublishEventsError>(crate::output::PublishEventsOutput {})
                            }).build_unchecked();
                        let response = #{Tower}::ServiceExt::oneshot(
                            service, request(vec![(Duration::ZERO, initial_frame())], true),
                        ).await.unwrap();
                        assert!(response.status().is_success(), "{:?}", response.status());
                        """,
                        *scope,
                    )
                }
                tokioTest("idle_deadline_after_initial_message_returns_resumable_error") {
                    rustTemplate(
                        """
                        use #{SmithyHttpServer}::request::FromRequest;
                        use crate::event_stream_message_timeout::MessageTimeoutError;
                        tokio::time::pause();
                        let mut input = crate::input::PublishEventsInput::from_request(
                            request(vec![(Duration::ZERO, initial_frame())], true),
                        ).await.unwrap();
                        let start = tokio::time::Instant::now();
                        let error = input.events.recv().await.unwrap_err();
                        let timeout = MessageTimeoutError::find_in_chain(&error).unwrap();
                        assert!(timeout.is_idle());
                        assert!(!timeout.is_initial_message());
                        assert_eq!(timeout.timeout(), Duration::from_secs(2));
                        assert_elapsed(start, 2000, 2001);
                        let error = input.events.recv().await.unwrap_err();
                        assert!(MessageTimeoutError::find_in_chain(&error).unwrap().is_idle());
                        assert_elapsed(start, 4000, 4002);
                        """,
                        *scope,
                    )
                }
            }
        }
    }
}
