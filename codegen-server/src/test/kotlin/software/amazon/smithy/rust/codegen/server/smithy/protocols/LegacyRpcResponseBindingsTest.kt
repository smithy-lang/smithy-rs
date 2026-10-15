/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

internal class LegacyRpcResponseBindingsTest {
    @Test
    fun `record legacy RPC response bindings before changing compatibility`() {
        val reportDirectory = Path.of("/tmp/legacy-rpc-response-bindings")
        Files.createDirectories(reportDirectory)
        Files.writeString(
            reportDirectory.resolve("observations.tsv"),
            "protocol\thttpVersion\tcase\tstatus\theaders\tbodyText\tbodyHex\n",
        )
        for ((namespace, protocol) in listOf(
            "aws.protocols" to "awsJson1_0",
            "aws.protocols" to "awsJson1_1",
            "smithy.protocols" to "rpcv2Cbor",
        )) {
            val errors = if (protocol == "rpcv2Cbor") "" else "errors: [Failure]"
            val modelText =
                """
                namespace test
                use $namespace#$protocol
                @$protocol
                service Example { version: "1" operations: [Write] }
                @http(method: "PUT", uri: "/things/{name}", code: 201)
                operation Write {
                    input := { @required @httpLabel name: String }
                    output: Out
                    $errors
                }
                structure Out {
                    name: String
                    @httpHeader("ETag") etag: String
                    @httpResponseCode code: Integer
                }
                @error("client")
                @httpError(422)
                structure Failure {
                    message: String
                    @httpHeader("ETag") etag: String
                    code: Integer
                }
                """.trimIndent()
            val model = modelText.asSmithyModel(smithyVersion = "2")
            Files.writeString(reportDirectory.resolve("$protocol.smithy"), modelText)
            val generated =
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":false}}""").expectObjectNode(),
                        cargoCommand = "cargo test --quiet --lib --tests",
                    ),
                    testCoverage = HttpTestType.ALL,
                ) { context, crate ->
                    check(!context.usesSchemaHttpSerde)
                    val write = context.model.expectShape(ShapeId.from("test#Write"), OperationShape::class.java)
                    val marker =
                        when (protocol) {
                            "awsJson1_0" -> "protocol::aws_json_10::AwsJson1_0"
                            "awsJson1_1" -> "protocol::aws_json_11::AwsJson1_1"
                            else -> "protocol::rpc_v2_cbor::RpcV2Cbor"
                        }
                    val recordError =
                        if (protocol == "rpcv2Cbor") {
                            ""
                        } else {
                            """
                            let error = #{WriteError}::Failure(crate::error::Failure {
                                message: #{Some}(#{String}::from("n")),
                                etag: #{Some}(#{String}::from("\"abc\"")),
                                code: #{Some}(299),
                            });
                            let response = #{Server}::response::IntoResponse::<#{Marker}>::into_response(error);
                            record("error", response).await;
                            """
                        }
                    crate.testModule {
                        tokioTest("record_generated_legacy_responses") {
                            rustTemplate(
                                """
                                async fn record(case: &str, response: #{Server}::response::Response) {
                                    use ::std::io::Write;
                                    let status = response.status().as_u16();
                                    let headers = format!("{:?}", response.headers());
                                    #{CollectBody}
                                    let text = #{String}::from_utf8_lossy(&body);
                                    let hex = body.iter().map(|byte| format!("{byte:02x}")).collect::<#{String}>();
                                    let mut report = ::std::fs::OpenOptions::new().append(true)
                                        .open("/tmp/legacy-rpc-response-bindings/observations.tsv").unwrap();
                                    writeln!(report, "$protocol\t${context.runtimeConfig.httpVersion}\t{case}\t{status}\t{headers}\t{text:?}\t{hex}").unwrap();
                                }
                                for code in [#{None}, #{Some}(299), #{Some}(-1)] {
                                    let output = #{Output} {
                                        name: #{Some}(#{String}::from("n")),
                                        etag: #{Some}(#{String}::from("\"abc\"")),
                                        code,
                                    };
                                    let response = #{Server}::response::IntoResponse::<#{Marker}>::into_response(output);
                                    record(&format!("output code={code:?}"), response).await;

                                }
                                $recordError
                                """,
                                "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                "Marker" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType().resolve(marker),
                                "Output" to context.symbolProvider.toSymbol(write.outputShape(context.model)),
                                "WriteError" to context.symbolProvider.symbolForOperationError(write),
                                "CollectBody" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                                *RuntimeType.preludeScope,
                            )
                        }
                    }
                }
            Files.writeString(
                reportDirectory.resolve("$protocol-generated.txt"),
                generated.joinToString("\n") { "${it.httpVersion}: ${it.path}" },
            )
        }
    }
}
