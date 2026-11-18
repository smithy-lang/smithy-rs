/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/** Keeps the legacy polling observation explicit alongside schema configuration tests. */
internal class RequestBodyReadPollBudgetTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace test
        use aws.protocols#restJson1
        @restJson1
        service Example { operations: [Echo, Upload, Events] }
        @http(method: "POST", uri: "/echo")
        operation Echo {
            input := { @required value: String }
            output := { @required value: String }
        }
        @http(method: "POST", uri: "/upload")
        operation Upload {
            input := { @required @httpPayload data: Data }
            output := { @required value: String }
        }
        @streaming
        blob Data
        @http(method: "POST", uri: "/events")
        operation Events {
            input := { @required @httpPayload events: EventStream }
            output := { @required value: String }
        }
        @streaming
        union EventStream { value: Value }
        structure Value { value: String }
        """.asSmithyModel()

    @Test
    fun `legacy reads ready bodies without a forced yield`() {
        checkGeneratedBudget(schema = false, budget = 0, expectedBudget = null)
    }

    @Test
    fun `schema baseline yields after 32 ready frames`() {
        checkGeneratedBudget(schema = true, budget = null, expectedBudget = 32)
    }

    @Test
    fun `schema collection and streaming share the configured budget`() {
        checkGeneratedBudget(schema = true, budget = 3, expectedBudget = 3)
    }

    @Test
    fun `schema zero disables forced yielding for collection and streaming`() {
        checkGeneratedBudget(schema = true, budget = 0, expectedBudget = null)
    }

    @Test
    fun `runtime body configuration overrides the generated poll budget`() {
        checkGeneratedBudget(schema = true, budget = 3, expectedBudget = null, runtimeOverride = true)
        checkGeneratedBudget(schema = true, budget = 0, expectedBudget = 7, runtimeOverride = true)
    }

    private fun checkGeneratedBudget(
        schema: Boolean,
        budget: Int?,
        expectedBudget: Int?,
        runtimeOverride: Boolean = false,
    ) {
        val common =
            budget?.let {
                ""","customizationConfig":{"protocols":{"common":{"request":{"body":{"readPollBudget":$it}}}}}"""
            } ?: ""
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = Node.parse("""{"codegen":{"schemaSerde":$schema}$common}""").expectObjectNode(),
                overrideTestDir = Files.createTempDirectory("request-body-poll-budget-").toFile(),
                cargoCommand = "cargo test --quiet --lib --tests -- --nocapture",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            crate.testModule {
                tokioTest("ready_body_polling_and_wire_results") {
                    rustTemplate(
                        """
                        use #{Tower}::ServiceExt;
                        use #{BodyUtil}::BodyExt;
                        use ::std::future::Future;
                        use ::std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
                        use ::std::task::{Context, Poll};
                        let service = crate::Example::builder(crate::ExampleConfig::builder().build())
                            .echo(|input: crate::input::EchoInput| async move {
                                #{Ok}::<_, crate::error::EchoError>(crate::output::EchoOutput { value: input.value })
                            })
                            .upload(|input: crate::input::UploadInput| async move {
                                let bytes = input.data.collect().await.unwrap().into_bytes();
                                assert_eq!(bytes, "ok");
                                #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput { value: "ok".to_owned() })
                            })
                            .events(|mut input: crate::input::EventsInput| async move {
                                let event = input.events.recv().await.unwrap().unwrap();
                                assert!(matches!(event, crate::model::EventStream::Value(value) if value.value.as_deref() == #{Some}("ok")));
                                assert!(input.events.recv().await.unwrap().is_none());
                                #{Ok}::<_, crate::error::EventsError>(crate::output::EventsOutput { value: "ok".to_owned() })
                            })
                            #{RuntimeConfig}
                            .build_unchecked();
                        let message = #{Types}::event_stream::Message::new(#{Bytes}::Bytes::from_static(br##"{"value":"ok"}"##))
                            .add_header(#{Types}::event_stream::Header::new(":message-type", #{Types}::event_stream::HeaderValue::String("event".into())))
                            .add_header(#{Types}::event_stream::Header::new(":event-type", #{Types}::event_stream::HeaderValue::String("value".into())))
                            .add_header(#{Types}::event_stream::Header::new(":content-type", #{Types}::event_stream::HeaderValue::String("application/json".into())));
                        let mut event_bytes = #{Vec}::new();
                        #{EventStream}::frame::write_message_to(&message, &mut event_bytes).unwrap();
                        for (uri, content_type, payload, status) in [
                            ("/echo", "application/json", #{Bytes}::Bytes::from_static(br##"{"value":"ok"}"##), 200),
                            ("/echo", "application/json", #{Bytes}::Bytes::from_static(b"{}"), 400),
                            ("/upload", "application/octet-stream", #{Bytes}::Bytes::from_static(b"ok"), 200),
                            ("/events", "application/vnd.amazon.eventstream", #{Bytes}::Bytes::from(event_bytes), 200),
                        ] {
                            let polls = Arc::new(AtomicUsize::new(0));
                            let observed = polls.clone();
                            let frames = #{Futures}::stream::poll_fn(move |_| {
                                let index = observed.fetch_add(1, Ordering::SeqCst);
                                Poll::Ready(match index {
                                    0 => #{Some}(#{Ok}::<_, ::std::convert::Infallible>(#{HttpBody}::Frame::data(payload.slice(..1)))),
                                    1..=64 => #{Some}(#{Ok}(#{HttpBody}::Frame::data(#{Bytes}::Bytes::new()))),
                                    65 => #{Some}(#{Ok}(#{HttpBody}::Frame::data(payload.slice(1..)))),
                                    _ => #{None},
                                })
                            });
                            let request = #{Http}::Request::builder().method("POST").uri(uri)
                                .header("content-type", content_type).body(#{BodyUtil}::StreamBody::new(frames)).unwrap();
                            let mut response = #{Box}::pin(service.clone().oneshot(request));
                            let mut cx = Context::from_waker(::std::task::Waker::noop());
                            let mut yields = 0;
                            let response = loop {
                                match response.as_mut().poll(&mut cx) {
                                    Poll::Ready(response) => break response.unwrap(),
                                    Poll::Pending => {
                                        yields += 1;
                                        assert_eq!(polls.load(Ordering::SeqCst), yields * ${expectedBudget ?: 0},
                                            "unexpected pending poll for {uri}");
                                        assert!(yields <= 67, "body must complete");
                                    }
                                }
                            };
                            let expected_polls = if uri == "/events" { 68 } else { 67 };
                            assert_eq!(polls.load(Ordering::SeqCst), expected_polls);
                            assert_eq!(yields, ${expectedBudget?.let { "(expected_polls - 1) / $it" } ?: "0"});
                            let (parts, body) = response.into_parts();
                            let body = body.collect().await.unwrap().to_bytes();
                            println!("schema=$schema budget=${budget ?: "default"} uri={uri} status={} headers={:?} body={:?} polls={} yields={yields}",
                                parts.status, parts.headers, body, polls.load(Ordering::SeqCst));
                            assert_eq!(parts.status, status);
                            assert_eq!(parts.headers["content-type"], "application/json");
                            if status == 200 {
                                assert_eq!(parts.headers["content-length"], "14");
                                assert_eq!(body, br##"{"value":"ok"}"##.as_slice());
                            } else {
                                assert_eq!(parts.headers["x-amzn-errortype"], "ValidationException");
                                // These field orders were observed separately in the legacy and schema baselines.
                                let expected = if $schema {
                                    br##"{"message":"1 validation error detected. Value at '/value' failed to satisfy constraint: Member must not be null","fieldList":[{"path":"/value","message":"Value at '/value' failed to satisfy constraint: Member must not be null"}]}"##.as_slice()
                                } else {
                                    br##"{"fieldList":[{"path":"/value","message":"Value at '/value' failed to satisfy constraint: Member must not be null"}],"message":"1 validation error detected. Value at '/value' failed to satisfy constraint: Member must not be null"}"##.as_slice()
                                };
                                assert_eq!(body, expected);
                            }
                        }
                        """,
                        *RuntimeType.preludeScope,
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                        "HttpBody" to CargoDependency.HttpBody1x.toType(),
                        "Bytes" to CargoDependency.Bytes.toType(),
                        "Futures" to CargoDependency.FuturesUtil.toType(),
                        "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                        "EventStream" to RuntimeType.smithyEventStream(context.runtimeConfig),
                        "RuntimeConfig" to
                            writable {
                                if (runtimeOverride) {
                                    rustTemplate(
                                        """
                                        .request_body_config(#{Server}::config::ServiceRequestBodyConfig::default()
                                            .with_global(#{Server}::config::RequestBodyCollectionConfig::default()
                                                .with_read_poll_budget(#{Budget})))
                                        """,
                                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                        "Budget" to
                                            writable {
                                                if (expectedBudget == null) {
                                                    rustTemplate("#{None}", *RuntimeType.preludeScope)
                                                } else {
                                                    rustTemplate(
                                                        "#{NonZeroUsize}::new(${expectedBudget}usize)",
                                                        "NonZeroUsize" to RuntimeType.std.resolve("num::NonZeroUsize"),
                                                    )
                                                }
                                            },
                                    )
                                }
                            },
                    )
                }
            }
        }
    }
}
