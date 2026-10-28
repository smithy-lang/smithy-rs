/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
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
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Replays malformed initial event-stream request frames against single-protocol legacy and
 * schema services. It checks the rejection status, headers, and body against the observed legacy
 * result and compares the two captures. The service remains single-protocol so the test isolates
 * initial-frame validation rather than multi-protocol request claiming.
 */
internal class InitialEventStreamFrameCompatibilityTest {
    @Test
    fun `malformed initial frames match legacy validation rejections`() {
        val run = Files.createTempDirectory("initial-frame-parity-")
        for (protocol in listOf("awsJson1_0", "awsJson1_1", "rpcv2Cbor")) {
            val namespace =
                if (protocol == "rpcv2Cbor") {
                    "smithy.protocols"
                } else {
                    "aws.protocols"
                }
            val responseMediaType =
                when (protocol) {
                    "awsJson1_0" -> "application/x-amz-json-1.0"
                    "awsJson1_1" -> "application/x-amz-json-1.1"
                    else -> "application/cbor"
                }
            val model =
                """
                ${'$'}version: "2"
                namespace test.initialframe
                use $namespace#$protocol
                @$protocol
                service Frames { version: "1", operations: [Read] }
                operation Read { input := { @httpPayload events: Events }, output := {} }
                @streaming union Events { note: Note }
                structure Note { text: String }
                """.asSmithyModel()
            for (schemaSerde in listOf(false, true)) {
                val mode =
                    if (schemaSerde) {
                        "schema"
                    } else {
                        "legacy"
                    }
                val observations = run.resolve("$protocol-$mode.txt")
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
                        cargoCommand = "cargo test --quiet --lib -- --nocapture",
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    val operation = context.model.expectShape(ShapeId.from("test.initialframe#Read"), OperationShape::class.java)
                    val output = "crate::output::ReadOutput::builder().build()"
                    val returned =
                        if (operation.errors.isEmpty()) {
                            output
                        } else {
                            "Ok::<_, crate::error::ReadError>($output)"
                        }
                    crate.testModule {
                        tokioTest("malformed_initial_frames") {
                            rustTemplate(
                                """
                                let app = crate::Frames::builder(crate::FramesConfig::builder().build())
                                    .read(|_: crate::input::ReadInput| async { panic!("malformed frame reached handler"); ##[allow(unreachable_code)] $returned }).build().unwrap();
                                let mut records = #{String}::new();
                                let message = #{Types}::event_stream::Message::new_from_parts(vec![
                                    #{Types}::event_stream::Header::new(":message-type", #{Types}::event_stream::HeaderValue::String("event".into())),
                                    #{Types}::event_stream::Header::new(":event-type", #{Types}::event_stream::HeaderValue::String("initial-request".into())),
                                    #{Types}::event_stream::Header::new(":content-type", #{Types}::event_stream::HeaderValue::String("${if (protocol == "rpcv2Cbor") {
                                    "application/cbor"
                                } else {
                                    "application/json"
                                }}".into())),
                                ], ${if (protocol == "rpcv2Cbor") {
                                    "b\"\\xbf\\xff\".as_slice()"
                                } else {
                                    "b\"{}\".as_slice()"
                                }});
                                let mut valid = #{Vec}::new();
                                #{EventStream}::frame::write_message_to(&message, &mut valid).unwrap();
                                let mut bad_header = valid.clone(); bad_header[8] ^= 1;
                                let mut bad_message = valid.clone(); let last = bad_message.len() - 1; bad_message[last] ^= 1;
                                for (case, payload) in [
                                    ("prelude-crc", bad_header),
                                    ("message-crc", bad_message),
                                    ("truncated-prelude", valid[..8].to_vec()),
                                    ("truncated-message", valid[..valid.len() - 1].to_vec()),
                                ] {
                                    let request = #{Http}::Request::post(${if (protocol == "rpcv2Cbor") {
                                    "\"/service/Frames/operation/Read\""
                                } else {
                                    "\"/\""
                                }})
                                        .header("content-type", "application/vnd.amazon.eventstream")
                                        ${if (protocol == "rpcv2Cbor") {
                                    ".header(\"smithy-protocol\", \"rpc-v2-cbor\")"
                                } else {
                                    ".header(\"x-amz-target\", \"Frames.Read\")"
                                }}
                                        .body(#{RequestBody}).unwrap();
                                    let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                    let status = response.status().as_u16();
                                    let headers = response.headers().clone();
                                    #{Collect}
                                    assert_eq!(status, 400);
                                    // Observed on the generated serde path, including both truncations.
                                    if ${!schemaSerde} {
                                        let mut expected_headers = #{Http}::HeaderMap::new();
                                        expected_headers.insert("content-type", #{Http}::HeaderValue::from_static("$responseMediaType"));
                                        assert_eq!(headers, expected_headers, "{case}");
                                        assert_eq!(body.as_ref(), b"response error", "{case}");
                                    }
                                    records.push_str(&format!("{case}: {status} {headers:?} {:?}\n", body.as_ref()));
                                }
                                ::std::fs::write(${observations.toString().dq()}, records).unwrap();
                                """,
                                *RuntimeType.preludeScope,
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                                "EventStream" to RuntimeType.smithyEventStream(context.runtimeConfig),
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                                "RequestBody" to ServerHttpTestHelpers.createBodyFromBytes(context, "payload"),
                                "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                            )
                        }
                    }
                }
            }
            assertEquals(Files.readString(run.resolve("$protocol-legacy.txt")), Files.readString(run.resolve("$protocol-schema.txt")), "$protocol: $run")
        }
    }
}
