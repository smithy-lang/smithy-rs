/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class EventStreamPollBudgetTest {
    private val model =
        """
        ${'$'}version: "2.0"
        namespace test
        use aws.protocols#restJson1
        @restJson1
        service EventService { operations: [PublishEvents] }
        @http(uri: "/publish", method: "POST")
        operation PublishEvents {
            input := {
                @httpPayload
                @required
                events: Events
            }
            output := {}
        }
        @streaming
        union Events { event: Event }
        structure Event { data: String }
        """.asSmithyModel()

    @Test
    fun `omitted poll budget inherits runtime defaults and accepts nonnegative integers`() {
        check(config("{}").requestBodyReadPollBudget == null)
        check(settings("{}").requestBodyReadPollBudget == null)
        for (budget in listOf(0, 1, 32, 64, Int.MAX_VALUE)) {
            check(config("""{"readPollBudget": $budget}""").requestBodyReadPollBudget == budget)
        }
    }

    @Test
    fun `invalid poll budgets fail code generation`() {
        for (value in listOf("-1", "1.5", "2147483648", "9223372036854775808", "\"32\"", "true", "null")) {
            val error = assertThrows<CodegenException> { config("""{"readPollBudget": $value}""") }
            check(error.message!!.contains("customizationConfig.protocols.common.request.body.readPollBudget"))
        }
    }

    @Test
    fun `generated event stream receiver yields by default without a timeout`() {
        checkGeneratedBudget("{}", 32, false)
    }

    @Test
    fun `generated event stream receiver uses configured budget with a timeout`() {
        checkGeneratedBudget(
            """
            {
                "customizationConfig": {
                    "protocols": {
                        "common": {
                            "request": {
                                "body": {
                                    "readPollBudget": 3,
                                    "eventStreamTimeouts": {"messageCompletion": "30s"}
                                }
                            }
                        }
                    }
                }
            }
            """,
            3,
            true,
        )
    }

    @Test
    fun `zero disables forced yielding in generated receiver`() {
        checkGeneratedBudget(
            """{"customizationConfig": {"protocols": {"common": {"request":{"body":{"readPollBudget": 0}}}}}}""",
            64,
            false,
        )
    }

    @Test
    fun `generated receiver reports clean EOF after idle expiry at a budget boundary`() {
        for (budget in listOf(1, 32)) {
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings =
                        Node.parse(
                            """
                            {"customizationConfig": {"protocols": {"common": {"request": {"body": {
                                "readPollBudget": $budget,
                                "eventStreamTimeouts": {"idle": "100ms"}
                            }}}}}}
                            """,
                        ).expectObjectNode(),
                ),
            ) { codegenContext, rustCrate ->
                rustCrate.testModule {
                    tokioTest("clean_eof_after_idle_expiry_at_budget_boundary") {
                        rustTemplate(
                            """
                            use #{SmithyHttpServer}::request::FromRequest;
                            use #{SmithyTypes}::event_stream::{Header, HeaderValue, Message};
                            use std::pin::Pin;
                            use std::task::{Context, Poll};
                            use std::time::Duration;

                            struct ReadyEvents { remaining: usize, frame: #{Bytes} }
                            impl #{Body1x}::Body for ReadyEvents {
                                type Data = #{Bytes};
                                type Error = std::convert::Infallible;
                                fn poll_frame(
                                    mut self: Pin<&mut Self>,
                                    _cx: &mut Context<'_>,
                                ) -> Poll<#{Option}<#{Result}<#{Body1x}::Frame<#{Bytes}>, Self::Error>>> {
                                    if self.remaining == 0 {
                                        return Poll::Ready(#{None});
                                    }
                                    self.remaining -= 1;
                                    Poll::Ready(#{Some}(#{Ok}(#{Body1x}::Frame::data(self.frame.clone()))))
                                }
                                // Use Body's default is_end_stream(): EOF must be polled.
                            }
                            tokio::time::pause();
                            let message = Message::new(#{Bytes}::from_static(b"{\"data\":\"hello\"}"))
                                .add_header(Header::new(":message-type", HeaderValue::String("event".into())))
                                .add_header(Header::new(":event-type", HeaderValue::String("event".into())))
                                .add_header(Header::new(":content-type", HeaderValue::String("application/json".into())));
                            let mut frame = #{Vec}::new();
                            #{EventStream}::frame::write_message_to(&message, &mut frame).unwrap();
                            let request = #{Http}::Request::builder()
                                .uri("/publish").method("POST")
                                .header("Content-Type", "application/vnd.amazon.eventstream")
                                .body(#{SdkBody}::from_body_1_x(ReadyEvents {
                                    remaining: $budget, frame: frame.into(),
                                })).unwrap();
                            let mut input = crate::input::PublishEventsInput::from_request(request).await.unwrap();
                            for _ in 0..$budget {
                                assert!(input.events.recv().await.unwrap().is_some());
                            }
                            tokio::time::advance(Duration::from_millis(200)).await;
                            assert!(input.events.recv().await.expect("clean EOF must not time out").is_none());
                            """,
                            *RuntimeType.preludeScope,
                            "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                            "SmithyTypes" to RuntimeType.smithyTypes(codegenContext.runtimeConfig),
                            "EventStream" to RuntimeType.smithyEventStream(codegenContext.runtimeConfig),
                            "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                            "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                            "Body1x" to CargoDependency.HttpBody1x.toType(),
                            "Bytes" to RuntimeType.Bytes,
                        )
                    }
                }
            }
        }
    }

    @Test
    fun `malformed common protocol configuration is rejected`() {
        for (json in listOf(
            """{"protocols": null}""",
            """{"protocols": []}""",
            """{"protocols": {"common": true}}""",
            """{"protocols": {"common": {"request":{"body":{"readTimeouts": null}}}}}""",
            """{"protocols": {"common": {"request":{"body":{"readTimeouts": "30s"}}}}}""",
        )) {
            val error = assertThrows<CodegenException> { settings("""{"customizationConfig": $json}""") }
            check(error.message!!.contains("customizationConfig.protocols"))
        }
    }

    private fun config(json: String): ServerRustSettings =
        settings("""{"customizationConfig": {"protocols": {"common": {"request":{"body":$json}}}}}""")

    private fun settings(json: String): ServerRustSettings =
        ServerRustSettings.from(
            model,
            Node.parse(
                """
                {
                    "service": "test#EventService",
                    "module": "test-service",
                    "moduleVersion": "0.0.1",
                    "moduleAuthors": ["test"]
                }
                """,
            ).expectObjectNode().merge(Node.parse(json).expectObjectNode()),
        )

    private fun checkGeneratedBudget(
        settings: String,
        expectedFrames: Int,
        withTimeout: Boolean,
    ) {
        serverIntegrationTest(
            model,
            IntegrationTestParams(additionalSettings = Node.parse(settings).expectObjectNode()),
        ) { codegenContext, rustCrate ->
            rustCrate.testModule {
                tokioTest("generated_receiver_yields_at_configured_budget") {
                    rustTemplate(
                        """
                        use crate::input::PublishEventsInput;
                        use #{SmithyHttpServer}::request::FromRequest;
                        use std::future::Future;
                        use std::pin::Pin;
                        use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
                        use std::task::{Context, Poll, Wake, Waker};

                        struct ReadyFrames { frames: Arc<AtomicUsize> }
                        impl #{Body1x}::Body for ReadyFrames {
                            type Data = #{Bytes};
                            type Error = std::convert::Infallible;
                            fn poll_frame(
                                self: Pin<&mut Self>,
                                _cx: &mut Context<'_>,
                            ) -> Poll<#{Option}<#{Result}<#{Body1x}::Frame<#{Bytes}>, Self::Error>>> {
                                let count = self.frames.load(Ordering::SeqCst);
                                if count == 64 {
                                    return Poll::Pending;
                                }
                                self.frames.fetch_add(1, Ordering::SeqCst);
                                // Partial event followed by empty ready frames: recv() must
                                // yield without waiting for a complete event or transport IO.
                                let bytes = if count == 0 {
                                    #{Bytes}::from_static(&[0, 0, 0, 32, 0, 0])
                                } else {
                                    #{Bytes}::new()
                                };
                                Poll::Ready(#{Some}(#{Ok}(#{Body1x}::Frame::data(bytes))))
                            }
                        }
                        struct CountWake(AtomicUsize);
                        impl Wake for CountWake {
                            fn wake(self: Arc<Self>) {
                                self.0.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                        let frames = Arc::new(AtomicUsize::new(0));
                        let request = #{Http}::Request::builder()
                            .uri("/publish").method("POST")
                            .header("Content-Type", "application/vnd.amazon.eventstream")
                            .body(#{SdkBody}::from_body_1_x(ReadyFrames { frames: frames.clone() }))
                            .unwrap();
                        let mut input = PublishEventsInput::from_request(request).await.unwrap();
                        assert_eq!(frames.load(Ordering::SeqCst), 0);
                        let wake = Arc::new(CountWake(AtomicUsize::new(0)));
                        let waker = Waker::from(wake.clone());
                        let mut cx = Context::from_waker(&waker);
                        let mut recv = #{Box}::pin(input.events.recv());
                        assert!(recv.as_mut().poll(&mut cx).is_pending());
                        assert_eq!(frames.load(Ordering::SeqCst), $expectedFrames);
                        assert_eq!(wake.0.load(Ordering::SeqCst), ${if (expectedFrames == 64) {
                            0
                        } else {
                            1
                        }});
                        """,
                        *RuntimeType.preludeScope,
                        "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                        "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                        "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                        "Body1x" to CargoDependency.HttpBody1x.toType(),
                        "Bytes" to RuntimeType.Bytes,
                    )
                }
                if (withTimeout) {
                    tokioTest("expired_timeout_precedes_data_after_a_forced_yield") {
                        rustTemplate(
                            """
                            use #{Body1x}::Body;
                            use std::pin::Pin;
                            use std::task::{Context, Poll, Waker};
                            struct ReadyPartial;
                            impl #{Body1x}::Body for ReadyPartial {
                                type Data = #{Bytes};
                                type Error = std::convert::Infallible;
                                fn poll_frame(
                                    self: Pin<&mut Self>,
                                    _cx: &mut Context<'_>,
                                ) -> Poll<#{Option}<#{Result}<#{Body1x}::Frame<#{Bytes}>, Self::Error>>> {
                                    Poll::Ready(#{Some}(#{Ok}(#{Body1x}::Frame::data(
                                        #{Bytes}::from_static(&[0, 0, 0, 32, 0, 0]),
                                    ))))
                                }
                            }
                            tokio::time::pause();
                            let body = crate::event_stream_message_timeout::wrap_with_message_timeout(
                                #{SdkBody}::from_body_1_x(ReadyPartial), std::time::Duration::from_millis(100),
                            );
                            let mut body = crate::event_stream_poll_budget::wrap_with_poll_budget(
                                body, 1,
                            );
                            let mut cx = Context::from_waker(Waker::noop());
                            assert!(matches!(Pin::new(&mut body).poll_frame(&mut cx), Poll::Ready(#{Some}(#{Ok}(_)))));
                            tokio::time::advance(std::time::Duration::from_millis(100)).await;
                            assert!(Pin::new(&mut body).poll_frame(&mut cx).is_pending());
                            match Pin::new(&mut body).poll_frame(&mut cx) {
                                Poll::Ready(#{Some}(#{Err}(err))) => {
                                    assert!(crate::event_stream_message_timeout::MessageTimeoutError::find_in_chain(err.as_ref()).is_some());
                                }
                                other => panic!("expired timeout must precede data after budget yield: {other:?}"),
                            }
                            """,
                            *RuntimeType.preludeScope,
                            "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                            "Body1x" to CargoDependency.HttpBody1x.toType(),
                            "Bytes" to RuntimeType.Bytes,
                        )
                    }
                }
            }
        }
    }
}
