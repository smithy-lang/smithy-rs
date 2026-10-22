/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Generates REST JSON and REST XML event-stream servers with initial response frames enabled
 * and disabled, using both serde modes. It checks frame headers, initial payloads, successful
 * events, and modeled exceptions. An idle application stream verifies that enabling the setting
 * sends an initial frame promptly without closing the stream; the short timeouts are observation
 * guards in the test, not configured server read or idle timeouts.
 */
internal class EventStreamInitialResponseCompatibilityTest {
    @Test
    fun `restJson1 initial response matches generated legacy servers`() = compare("restJson1", "application/json", "{}")

    @Test
    fun `restXml initial response matches generated legacy servers`() = compare("restXml", "application/xml", "")

    private fun compare(
        protocol: String,
        mediaType: String,
        initialPayload: String,
    ) {
        val model =
            """
            ${'$'}version: "2"
            namespace compat.initial
            use aws.protocols#$protocol
            @$protocol
            service WatchService { version: "1", operations: [Watch] }
            @http(method: "POST", uri: "/watch")
            operation Watch { input: WatchInput, output: WatchOutput }
            structure WatchInput { @httpHeader("x-idle") idle: Boolean }
            structure WatchOutput {
                @httpHeader("x-session-id") sessionId: String
                @httpPayload events: Events
            }
            @streaming union Events { update: Update, failure: StreamFailure }
            structure Update { message: String }
            @error("server") structure StreamFailure { message: String }
            """.asSmithyModel()
        val snapshots = Files.createTempDirectory("smithy-initial-response-$protocol-")
        for (sendInitial in listOf(false, true)) {
            for (schemaSerde in listOf(false, true)) {
                val snapshot = snapshots.resolve("$sendInitial-$schemaSerde.txt")
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        overrideTestDir = snapshots.resolve("server-$sendInitial-$schemaSerde").toFile(),
                        additionalSettings =
                            ServerAdditionalSettings.builder().schemaSerde(schemaSerde).alwaysSendEventStreamInitialResponse(sendInitial).toObjectNode(),
                        cargoCommand = "cargo test --quiet --lib --tests",
                    ),
                    testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
                ) { context, crate ->
                    check(context.usesSchemaHttpSerde == schemaSerde)
                    crate.testModule {
                        tokioTest("initial_response_matches_legacy_on_idle_events_and_errors") {
                            rustTemplate(
                                """
                                use #{Tower}::ServiceExt;
                                use #{BodyUtil}::BodyExt;
                                let service = crate::WatchService::builder(crate::WatchServiceConfig::builder().build())
                                    .watch(|input: crate::input::WatchInput| async move {
                                        let events: #{EventStreamSender}<crate::model::Events, crate::error::EventsError> =
                                            if input.idle.unwrap_or(false) {
                                                #{Futures}::stream::pending().into()
                                            } else {
                                                #{Futures}::stream::iter([
                                                    #{Ok}(crate::model::Events::Update(crate::model::Update {
                                                        message: #{Some}("hello".into()),
                                                    })),
                                                    #{Err}(crate::error::EventsError::StreamFailure(crate::error::StreamFailure {
                                                        message: #{Some}("broken".into()),
                                                    })),
                                                ]).into()
                                            };
                                        #{Ok}::<_, crate::error::WatchError>(crate::output::WatchOutput {
                                            session_id: #{Some}("session-123".into()), events,
                                        })
                                    }).build_unchecked();
                                let mut snapshots = #{String}::new();
                                for idle in [false, true] {
                                    let request = #{Http}::Request::builder().method("POST").uri("/watch")
                                        .header("content-type", ${mediaType.dq()})
                                        .header("x-idle", if idle { "true" } else { "false" })
                                        .body(#{Server}::body::empty_sync()).unwrap();
                                    let response = service.clone().oneshot(request).await.unwrap();
                                    assert_eq!(response.status(), #{Http}::StatusCode::OK);
                                    assert_eq!(response.headers()["x-session-id"], "session-123");
                                    assert_eq!(response.headers()["content-type"], "application/vnd.amazon.eventstream");
                                    let (parts, mut body) = response.into_parts();
                                    let mut headers = parts.headers.iter().map(|(name, value)|
                                        (name.as_str().to_owned(), value.as_bytes().to_vec())).collect::<#{Vec}<_>>();
                                    headers.sort();
                                    snapshots.push_str(&format!("idle={idle} {} {headers:?}\n", parts.status));
                                    let send_initial = $sendInitial;
                                    let bytes = if idle {
                                        let first = #{Tokio}::time::timeout(
                                            std::time::Duration::from_millis(100), body.frame()
                                        ).await;
                                        if send_initial {
                                            let bytes = first.expect("initial frame must arrive while the application stream is idle")
                                                .unwrap().unwrap().into_data().unwrap();
                                            // The initial response must not close the application stream.
                                            assert!(#{Tokio}::time::timeout(
                                                std::time::Duration::from_millis(100), body.frame()
                                            ).await.is_err());
                                            bytes
                                        } else {
                                            assert!(first.is_err(), "disabled setting must leave an idle stream pending");
                                            #{Bytes}::new()
                                        }
                                    } else {
                                        body.collect().await.unwrap().to_bytes()
                                    };
                                    let mut remaining = bytes.as_ref();
                                    let mut frames = #{Vec}::new();
                                    while !remaining.is_empty() {
                                        frames.push(#{EventStream}::frame::read_message_from(&mut remaining).unwrap());
                                    }
                                    assert_eq!(frames.len(), usize::from(send_initial) + if idle { 0 } else { 2 });
                                    if send_initial {
                                        // Verified generated serde baseline: REST metadata remains in HTTP headers;
                                        // initial-response is {} for REST JSON and zero bytes for REST XML.
                                        let initial = &frames[0];
                                        assert_eq!(initial.headers().len(), 3);
                                        for (name, value) in [
                                            (":message-type", "event"), (":event-type", "initial-response"),
                                            (":content-type", ${mediaType.dq()}),
                                        ] {
                                            assert_eq!(initial.headers().iter().find(|h| h.name().as_str() == name)
                                                .unwrap().value().as_string().unwrap().as_str(), value);
                                        }
                                        assert_eq!(initial.payload().as_ref(), ${initialPayload.dq()}.as_bytes());
                                    }
                                    if !idle {
                                        let event = &frames[usize::from(send_initial)];
                                        assert_eq!(event.headers().iter().find(|h| h.name().as_str() == ":event-type")
                                            .unwrap().value().as_string().unwrap().as_str(), "update");
                                        let error = &frames[usize::from(send_initial) + 1];
                                        assert_eq!(error.headers().iter().find(|h| h.name().as_str() == ":message-type")
                                            .unwrap().value().as_string().unwrap().as_str(), "exception");
                                    }
                                    // Compare complete frame bytes (including CRCs), HTTP status and headers.
                                    snapshots.push_str(&format!("{bytes:02x?}\n"));
                                }
                                std::fs::write(${snapshot.toString().dq()}, snapshots).unwrap();
                                """,
                                *preludeScope,
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                                "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                                "Futures" to CargoDependency.FuturesUtil.toType(),
                                "EventStream" to CargoDependency.smithyEventStream(context.runtimeConfig).toType(),
                                "EventStreamSender" to RuntimeType.smithyHttp(context.runtimeConfig).resolve("event_stream::EventStreamSender"),
                                "Bytes" to RuntimeType.Bytes,
                                "Tokio" to ServerCargoDependency.TokioDev.toType(),
                            )
                        }
                    }
                }
            }
            Files.readString(snapshots.resolve("$sendInitial-true.txt")) shouldBe
                Files.readString(snapshots.resolve("$sendInitial-false.txt"))
        }
    }
}
