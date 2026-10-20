/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
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

internal class SingleProtocolRoutingCompatibilityTest {
    @Test
    fun `single protocol routing rejection responses match legacy`() {
        val observations = Files.createTempDirectory("single-protocol-routing-")
        for (protocol in listOf("restJson1", "restXml", "awsJson1_0", "awsJson1_1", "rpcv2Cbor")) {
            val uri =
                if (protocol == "rpcv2Cbor") {
                    "/service/Routes/operation/Read"
                } else if (protocol.startsWith("awsJson")) {
                    "/"
                } else {
                    "/read"
                }
            val contentType =
                when (protocol) {
                    "restXml" -> "application/xml"
                    "awsJson1_0" -> "application/x-amz-json-1.0"
                    "awsJson1_1" -> "application/x-amz-json-1.1"
                    "rpcv2Cbor" -> "application/cbor"
                    else -> "application/json"
                }
            val model =
                """
                ${'$'}version: "2"
                namespace test.routingcompat
                use ${if (protocol == "rpcv2Cbor") "smithy.protocols" else "aws.protocols"}#$protocol
                @$protocol service Routes { version: "1", operations: [Read] }
                @http(method: "POST", uri: "/read")
                operation Read { input := {}, output := {} }
                """.asSmithyModel()
            for (schemaSerde in listOf(false, true)) {
                val snapshot = observations.resolve("$protocol-$schemaSerde.txt")
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings = ObjectNode.parse("""{"codegen":{"http-1x":true,"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                        cargoCommand = "cargo test --quiet --lib",
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    crate.testModule {
                        tokioTest("routing_rejections") {
                            rustTemplate(
                                """
                                let app = crate::Routes::builder(crate::RoutesConfig::builder().build())
                                    .read(|_: crate::input::ReadInput| async { crate::output::ReadOutput::builder().build() })
                                    .build().unwrap();
                                let mut records = #{String}::new();
                                for (name, method, path, identified, target) in [
                                    ("known", "POST", ${uri.dq()}, true, "Routes.Read"),
                                    ("missing", "POST", "/no/such/operation", true, "Routes.Missing"),
                                    ("method", "GET", ${uri.dq()}, true, "Routes.Read"),
                                    ("unidentified", "POST", ${uri.dq()}, false, ""),
                                    ("unidentified-missing", "POST", "/no/such/operation", false, ""),
                                ] {
                                    let _ = target;
                                    let mut request = #{Http}::Request::builder().method(method).uri(path);
                                    if identified {
                                        request = request.header("content-type", ${contentType.dq()});
                                        ${if (protocol == "rpcv2Cbor") {
                                    """request = request.header("smithy-protocol", "rpc-v2-cbor");"""
                                } else if (protocol.startsWith("awsJson")) {
                                    """request = request.header("x-amz-target", target);"""
                                } else {
                                    "let _ = target;"
                                }}
                                    }
                                    let request = request.body(#{Server}::body::to_boxed_sync("")).unwrap();
                                    let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                    let status = response.status().as_u16();
                                    if name == "known" { assert_eq!(status, 200); }
                                    let mut headers: #{Vec}<_> = response.headers().iter()
                                        .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string())).collect();
                                    headers.sort();
                                    #{Collect}
                                    records.push_str(&format!("{name}: {status} {headers:?} {:?}\n", body.as_ref()));
                                }
                                ::std::fs::write(${snapshot.toString().dq()}, records).unwrap();
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
            assertEquals(Files.readString(observations.resolve("$protocol-false.txt")), Files.readString(observations.resolve("$protocol-true.txt")), "$protocol: $observations")
        }
    }
}
