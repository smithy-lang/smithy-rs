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

internal class RpcV2CborListPayloadCompatibilityTest {
    @Test
    fun `list payload members use the legacy keyed CBOR document`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.payloadsupport
            use smithy.protocols#rpcv2Cbor
            @rpcv2Cbor service Payloads { version: "1", operations: [Count] }
            @http(method: "POST", uri: "/count")
            operation Count {
                input := { @required @httpPayload values: Strings }
                output := { @required count: Integer }
            }
            list Strings { member: String }
            """.asSmithyModel()
        val observations = Files.createTempDirectory("cbor-list-payload-")
        for (schemaSerde in listOf(false, true)) {
            val snapshot = observations.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"http-1x":true,"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                    overrideTestDir = observations.resolve("$schemaSerde").toFile(),
                    cargoCommand = "cargo test --quiet --lib",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                crate.testModule {
                    tokioTest("list_payload_uses_a_keyed_document") {
                        rustTemplate(
                            """
                            let app = crate::Payloads::builder(crate::PayloadsConfig::builder().build())
                                .count(|input: crate::input::CountInput| async move {
                                    #{Ok}::<_, crate::error::CountError>(crate::output::CountOutput { count: input.values.len() as i32 })
                                }).build().unwrap();
                            let request = #{Http}::Request::post("/service/Payloads/operation/Count")
                                .header("content-type", "application/cbor")
                                .header("smithy-protocol", "rpc-v2-cbor")
                                .body(#{Server}::body::to_boxed_sync(
                                    vec![0xa1u8, 0x66, b'v', b'a', b'l', b'u', b'e', b's', 0x82, 0x61, b'a', 0x61, b'b']
                                )).unwrap();
                            let response = #{Tower}::ServiceExt::oneshot(app, request).await.unwrap();
                            let status = response.status().as_u16();
                            assert_eq!(status, 200);
                            let mut headers: #{Vec}<_> = response.headers().iter()
                                .map(|(key, value)| (key.to_string(), value.as_bytes().to_vec())).collect();
                            headers.sort();
                            #{Collect}
                            assert_eq!(body.as_ref(), &[0xbf, 0x65, b'c', b'o', b'u', b'n', b't', 2, 0xff]);
                            let record = format!("{status} {headers:?} {:?}", body.as_ref());
                            ::std::fs::write(${snapshot.toString().dq()}, record).unwrap();
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
        assertEquals(Files.readString(observations.resolve("false.txt")), Files.readString(observations.resolve("true.txt")))
    }
}
