/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

internal class RequiredMemberNullCompatibilityTest {
    // Legacy REST JSON writes fieldList before message; schema writes them in model order.
    // Preserve the raw captures, but compare validation errors as JSON objects.
    // Status and every header stay exact, as do successful and RPC response bytes.
    private fun responses(
        path: Path,
        protocol: String,
    ) = Files.readAllLines(path).map { record ->
        if (protocol == "restJson1" && record.contains(": 400 ") && record.contains("\"x-amzn-errortype\": \"ValidationException\"")) {
            val bodyStart = record.lastIndexOf(" [")
            val body =
                record.substring(bodyStart + 1).removeSurrounding("[", "]")
                    .split(", ").map { it.toInt().toByte() }.toByteArray()
            record.substring(0, bodyStart) to Node.parse(body.toString(Charsets.UTF_8))
        } else {
            record to null
        }
    }

    @Test fun `required null absent and duplicate values match legacy responses`() {
        val run = java.nio.file.Files.createTempDirectory("required-null-parity-")
        for (protocol in listOf("restJson1", "awsJson1_0", "awsJson1_1")) {
            for (schemaSerde in listOf(false, true)) {
                val observation = run.resolve("$protocol-$schemaSerde.txt")
                val model =
                    """
                    ${'$'}version: "2"
                    namespace test.review
                    use aws.protocols#$protocol
                    @$protocol
                    service Review { version: "1", operations: [Read] }
                    @http(method: "POST", uri: "/read")
                    operation Read { input := { @required text: String }, output := {} }
                    """.asSmithyModel()
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings = ObjectNode.parse("""{"codegen":{"http-1x":true,"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                        cargoCommand = "cargo test --quiet --lib -- --nocapture",
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    crate.testModule {
                        tokioTest("required_null_baseline") {
                            rustTemplate(
                                """
                                let app = crate::Review::builder(crate::ReviewConfig::builder().build())
                                    .read(|_: crate::input::ReadInput| async { Ok::<_, crate::error::ReadError>(crate::output::ReadOutput::builder().build()) }).build().unwrap();
                                let mut records = #{String}::new();
                                for input in ["{}", "{\"text\":null}", "{\"text\":\"ok\"}", "{\"text\":\"ok\",\"text\":null}"] {
                                    let request = #{Http}::Request::post(${if (protocol == "restJson1") "\"/read\"" else "\"/\""})
                                        .header("content-type", ${if (protocol == "restJson1") "\"application/json\"" else "\"application/x-amz-json-${if (protocol == "awsJson1_0") "1.0" else "1.1"}\""})
                                        .header("x-amz-target", "Review.Read")
                                        .body(#{Server}::body::to_boxed_sync(input)).unwrap();
                                    let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                    let status = response.status().as_u16();
                                    let headers = response.headers().clone();
                                    #{Collect}
                                    records.push_str(&format!("{input:?}: {status} {headers:?} {:?}\n", body.as_ref()));
                                    assert_eq!(status, if input.contains("ok") { 200 } else { 400 });
                                    if !input.contains("ok") { assert!(#{String}::from_utf8_lossy(&body).contains("ValidationException") || headers.get("x-amzn-errortype").is_some()); }
                                }
                                ::std::fs::write(${observation.toString().dq()}, records).unwrap();
                                """,
                                *RuntimeType.preludeScope,
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                                "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                            )
                        }
                    }
                }
            }
            assertEquals(responses(run.resolve("$protocol-false.txt"), protocol), responses(run.resolve("$protocol-true.txt"), protocol), "$protocol: $run")
        }
    }
}
