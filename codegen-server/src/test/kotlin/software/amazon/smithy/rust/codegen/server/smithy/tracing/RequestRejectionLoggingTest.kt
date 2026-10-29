/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.tracing

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Tag
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Sends the same valid, missing-required-member, and malformed CBOR requests to independently
 * generated legacy and schema servers. Rejections must produce DEBUG logs with usable error
 * diagnostics without enabling unredacted-logging, and must not call the handler. Legacy emits
 * two rejection events and schema emits one; codec wording and validation error formatting also
 * differ, so log strings are checked separately. Status, headers, response bytes, runtime error
 * extensions, and handler inputs must match across serde modes.
 */
@Tag("server-compatibility")
internal class RequestRejectionLoggingTest {
    @Test
    fun `generated request rejection logs retain diagnostics in both serde modes`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.requestlogging
            use smithy.protocols#rpcv2Cbor
            @rpcv2Cbor
            service LoggingService { version: "1", operations: [PutItem] }
            operation PutItem {
                input := { item: Item }
                output := {}
                errors: [smithy.framework#ValidationException]
            }
            structure Item { @required name: String }
            """.asSmithyModel()
        val observations = Files.createTempDirectory("request-rejection-logging-")
        for (schemaSerde in listOf(false, true)) {
            val snapshot = observations.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
                    overrideTestDir = observations.resolve("$schemaSerde").toFile(),
                    cargoCommand = "cargo test --quiet --lib -- --nocapture",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                check(context.usesSchemaHttpSerde == schemaSerde)
                crate.testModule {
                    tokioTest("request_rejection_diagnostics") {
                        rustTemplate(
                            """
                            use std::sync::{Arc, Mutex};
                            use #{Tracing}::instrument::WithSubscriber;

                            ##[derive(Clone, Default)]
                            struct LogWriter(Arc<Mutex<#{Vec}<u8>>>);
                            impl std::io::Write for LogWriter {
                                fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                                    self.0.lock().unwrap().extend_from_slice(bytes);
                                    #{Ok}(bytes.len())
                                }
                                fn flush(&mut self) -> std::io::Result<()> { #{Ok}(()) }
                            }

                            let seen = Arc::new(Mutex::new(#{None}));
                            let record = seen.clone();
                            let app = crate::LoggingService::builder(crate::LoggingServiceConfig::builder().build())
                                .put_item(move |input: crate::input::PutItemInput| {
                                    *record.lock().unwrap() = #{Some}(format!("{input:?}"));
                                    async { #{Ok}::<_, crate::error::PutItemError>(crate::output::PutItemOutput {}) }
                                }).build().unwrap();
                            // {"item":{"name":"a"}}, {"item":{}}, {}, and {"item":{"name":12}}.
                            let valid = vec![0xa1, 0x64, b'i', b't', b'e', b'm', 0xa1, 0x64, b'n', b'a', b'm', b'e', 0x61, b'a'];
                            let missing = vec![0xa1, 0x64, b'i', b't', b'e', b'm', 0xa0];
                            let wrong_type = vec![0xa1, 0x64, b'i', b't', b'e', b'm', 0xa1, 0x64, b'n', b'a', b'm', b'e', 12];
                            let mut truncated = valid.clone();
                            truncated.pop();
                            let cases = [
                                ("valid", valid, 200, ""),
                                ("missing name", missing, 400, ""),
                                ("item omitted", vec![0xa0], 200, ""),
                                ("wrong name type", wrong_type, 400, "expected text"),
                                ("truncated", truncated, 400, "end of input bytes"),
                                ("invalid", vec![0xff], 400, "expected map"),
                            ];
                            let mut observations = #{String}::new();
                            for (label, body, expected_status, diagnostic) in cases {
                                *seen.lock().unwrap() = #{None};
                                let writer = LogWriter::default();
                                let sink = writer.clone();
                                let subscriber = #{Subscriber}::fmt()
                                    .with_ansi(false).without_time().with_max_level(#{Tracing}::Level::DEBUG)
                                    .with_writer(move || sink.clone()).finish();
                                let request = #{Http}::Request::post("/service/LoggingService/operation/PutItem")
                                    .header("content-type", "application/cbor")
                                    .header("smithy-protocol", "rpc-v2-cbor")
                                    .body(#{Server}::body::to_boxed_sync(body)).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request)
                                    .with_subscriber(subscriber).await.unwrap();
                                let logs = #{String}::from_utf8(writer.0.lock().unwrap().clone()).unwrap();
                                let failures: #{Vec}<_> = logs.lines()
                                    .filter(|line| line.contains("failed to deserialize request")).collect();
                                assert_eq!(response.status().as_u16(), expected_status, "{label}: {logs}");
                                assert!(!logs.contains("error={redacted}"), "{label}: {logs}");
                                let handler_input = seen.lock().unwrap().clone();
                                if expected_status == 400 {
                                    assert!(handler_input.is_none(), "{label}: handler received {handler_input:?}");
                                    assert_eq!(failures.len(), if $schemaSerde { 1 } else { 2 }, "{label}: {logs}");
                                    assert!(failures.iter().all(|line| line.contains("DEBUG") && line.contains("error=")), "{label}: {logs}");
                                    assert!(logs.contains("failed to deserialize request into operation's input"), "{label}: {logs}");
                                    if label == "missing name" {
                                        if $schemaSerde {
                                            assert!(logs.contains("/item/name") && logs.contains("Member must not be null"), "{logs}");
                                        } else {
                                            // Legacy Display contains the serialized validation response as CBOR bytes.
                                            assert!(logs.contains("validation failure:"), "{logs}");
                                        }
                                    } else {
                                        assert!(logs.contains(diagnostic), "{label}: {logs}");
                                    }
                                } else {
                                    assert!(handler_input.is_some(), "{label}: handler was not called");
                                    assert!(failures.is_empty(), "{label}: {logs}");
                                }
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(key, value)| (key.to_string(), value.as_bytes().to_vec())).collect();
                                headers.sort();
                                let runtime_error = response.extensions()
                                    .get::<#{Server}::extension::RuntimeErrorExtension>().map(|error| error.to_string());
                                #{Collect}
                                observations.push_str(&format!("{label}|{expected_status}|{headers:?}|{body:?}|{runtime_error:?}|{handler_input:?}\n"));
                            }
                            std::fs::write(${snapshot.toString().dq()}, observations).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tracing" to CargoDependency.Tracing.toType(),
                            "Subscriber" to CargoDependency.TracingSubscriber.toType(),
                            "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                        )
                    }
                }
            }
        }
        assertEquals(Files.readString(observations.resolve("false.txt")), Files.readString(observations.resolve("true.txt")))
    }
}
