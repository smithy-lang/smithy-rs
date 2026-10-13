/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class EventStreamTimeoutsTest {
    private fun model(protocol: String = "aws.protocols#awsJson1_1") =
        """
        ${'$'}version: "2.0"
        namespace test
        use $protocol
        @${protocol.substringAfter("#")}
        service EventService { operations: [PublishEvents, Echo] }
        operation PublishEvents {
            input := {
                @required
                events: Events
                ${if (protocol == "smithy.protocols#rpcv2Cbor") "metadata: String" else ""}
            }
            output := {}
        }
        operation Echo {
            input := {}
            output := {}
        }
        @streaming
        union Events { event: Event }
        structure Event { data: String }
        """.asSmithyModel()

    private val publish = ShapeId.from("test#PublishEvents")
    private val echo = ShapeId.from("test#Echo")

    private fun settings(
        global: String,
        legacy: String? = null,
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
                                ${if (legacy != null) ""","requestBodyReadTimeouts": $legacy""" else ""}
                            }
                        }
                    }
                }
                """,
            ).expectObjectNode(),
        )

    @Test
    fun `initial and completion values are independent and disabled by default`() {
        val defaults = settings("{}").eventStreamTimeouts
        check(defaults.initialMessageMillisFor(publish) == null)
        check(defaults.messageCompletionMillisFor(publish) == null)
        check(defaults.idleMillisFor(publish) == 3_600_000L)
        check(defaults.idleMillisFor(echo) == null)
        val initialOnly = settings("""{"initialMessage": "10s"}""").eventStreamTimeouts
        check(initialOnly.initialMessageMillisFor(publish) == 10_000L)
        check(initialOnly.messageCompletionMillisFor(publish) == null)
        val completionOnly = settings("""{"messageCompletion": "30s"}""").eventStreamTimeouts
        check(completionOnly.initialMessageMillisFor(publish) == null)
        check(completionOnly.messageCompletionMillisFor(publish) == 30_000L)
        check(initialOnly.initialMessageMillisFor(echo) == null)
        check(completionOnly.messageCompletionMillisFor(echo) == null)
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
        check(timeouts.initialMessageMillisFor(publish) == null)
        check(timeouts.messageCompletionMillisFor(publish) == 5_000L)
        val fallback =
            settings(
                """{"initialMessage": "10s", "messageCompletion": "30s", "perOperation": {"test#PublishEvents": {"messageCompletion": 0}}}""",
            ).eventStreamTimeouts
        check(fallback.initialMessageMillisFor(publish) == 10_000L)
        check(fallback.messageCompletionMillisFor(publish) == null)
    }

    @Test
    fun `new event timeout object supersedes legacy completion settings`() {
        val legacy = """{"defaultEventStreamMessage": "45s", "perOperation": {"test#PublishEvents": "5s"}}"""
        val configured = settings("""{"initialMessage": "10s", "messageCompletion": "30s"}""", legacy).eventStreamTimeouts
        check(configured.initialMessageMillisFor(publish) == 10_000L)
        check(configured.messageCompletionMillisFor(publish) == 30_000L)
        val disabled = settings("{}", legacy).eventStreamTimeouts
        check(disabled.initialMessageMillisFor(publish) == null)
        check(disabled.messageCompletionMillisFor(publish) == null)
        val serviceSettings = settings("{}", legacy)
        val fallback = EventStreamTimeouts.fromCustomizationConfig(null, serviceSettings.requestBodyReadTimeouts)
        check(fallback.initialMessageMillisFor(publish) == null)
        check(fallback.messageCompletionMillisFor(publish) == 5_000L)
        check(fallback.idleMillisFor(publish) == 3_600_000L)
    }

    @Test
    fun `idle timeout can be configured overridden and disabled independently`() {
        val global = settings("""{"idle": "5m"}""").eventStreamTimeouts
        check(global.idleMillisFor(publish) == 300_000L)
        check(global.initialMessageMillisFor(publish) == null)
        check(global.messageCompletionMillisFor(publish) == null)
        check(settings("""{"idle": 0}""").eventStreamTimeouts.idleMillisFor(publish) == null)
        val disabled =
            settings(
                """{"idle": "5m", "perOperation": {"test#PublishEvents": {"idle": 0}}}""",
            ).eventStreamTimeouts
        check(disabled.idleMillisFor(publish) == null)
        val override =
            settings(
                """{"idle": 0, "perOperation": {"test#PublishEvents": {"idle": "10s"}}}""",
            ).eventStreamTimeouts
        check(override.idleMillisFor(publish) == 10_000L)
        val inherited =
            settings(
                """{"idle": "5m", "perOperation": {"test#PublishEvents": {"messageCompletion": "10s"}}}""",
            ).eventStreamTimeouts
        check(inherited.idleMillisFor(publish) == 300_000L)
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
            """{"initial": "10s"}""",
            """{"perOperation": null}""",
            """{"perOperation": {"test#Echo": {"initialMessage": "10s"}}}""",
            """{"perOperation": {"test#NotAttached": {"messageCompletion": "10s"}}}""",
            """{"perOperation": {"not a shape ID": {}}}""",
            """{"perOperation": {"test#PublishEvents": "10s"}}""",
            """{"perOperation": {"test#PublishEvents": {"completion": "10s"}}}""",
        )) {
            val error = assertThrows<CodegenException> { settings(json) }
            check(error.message!!.contains("customizationConfig.protocols.common.eventStreamTimeouts"))
        }
    }

    @Test
    fun `rpc v2 cbor initial and event deadlines work in generated code`() {
        checkRpcDeadlines("smithy.protocols#rpcv2Cbor")
    }

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
