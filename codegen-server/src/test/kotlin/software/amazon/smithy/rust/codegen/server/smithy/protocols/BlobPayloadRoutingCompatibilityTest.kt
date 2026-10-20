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

internal class BlobPayloadRoutingCompatibilityTest {
    @Test
    fun `blob content types preserve single protocol behavior and determine multi protocol routing`() {
        val observations = Files.createTempDirectory("untyped-blob-routing-")
        System.err.println("Untyped blob routing observations: $observations")
        for ((schemaSerde, multiProtocol) in listOf(false to false, true to false, true to true)) {
            val model =
                """
                ${'$'}version: "2"
                namespace test.uploadrouting
                use aws.protocols#restJson1
                @restJson1
                ${if (multiProtocol) "@smithy.protocols#rpcv2Cbor" else ""}
                service Uploads { version: "1", operations: [Upload] }
                @http(method: "POST", uri: "/upload")
                operation Upload {
                    input := { @required @httpPayload data: Blob }
                    output := { @required size: Integer }
                }
                """.asSmithyModel()
            val snapshot = observations.resolve("$schemaSerde-$multiProtocol.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"http-1x":true,"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                    overrideTestDir = observations.resolve("$schemaSerde-$multiProtocol").toFile(),
                    cargoCommand = "cargo test --quiet --lib -- --nocapture",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                crate.testModule {
                    tokioTest("upload_wire_observations") {
                        rustTemplate(
                            """
                            let app = crate::Uploads::builder(crate::UploadsConfig::builder().build())
                                .upload(|input: crate::input::UploadInput| async move {
                                    #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput { size: input.data.as_ref().len() as i32 })
                                }).build().unwrap();
                            let mut records = #{String}::new();
                            for content_type in [#{Some}("video/mp4"), #{Some}("application/octet-stream"), #{None}] {
                                let mut request = #{Http}::Request::post("/upload");
                                if let #{Some}(value) = content_type { request = request.header("content-type", value); }
                                let request = request.body(#{Server}::body::to_boxed_sync("abc")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                // Pinned after running the legacy and schema configurations.
                                let expected_status = if $multiProtocol && content_type != #{Some}("application/octet-stream") { 404 } else { 200 };
                                assert_eq!(status, expected_status, "{content_type:?}");
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(key, value)| (key.to_string(), value.as_bytes().to_vec())).collect();
                                headers.sort();
                                #{Collect}
                                if status == 200 {
                                    assert_eq!(body.as_ref(), b"{\"size\":3}");
                                } else {
                                    assert!(headers.is_empty());
                                    assert_eq!(body.as_ref(), b"<UnknownOperationException/>\n");
                                }
                                let record = format!("{content_type:?}: {status} {headers:?} {:?}\n", body.as_ref());
                                print!("{record}");
                                records.push_str(&record);
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
        assertEquals(Files.readString(observations.resolve("false-false.txt")), Files.readString(observations.resolve("true-false.txt")))
    }
}
