/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.routing

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
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
 * Uploads raw abc bytes to a Blob payload with no modeled media type, using absent, arbitrary,
 * and application/octet-stream Content-Type values. Legacy and schema REST-only services must
 * both accept all three and return the blob size. A schema REST JSON plus CBOR service must
 * accept application/octet-stream and return the pinned 404 fallback for the other two values.
 * The test records this multi-protocol routing difference explicitly rather than treating it
 * as a serde compatibility result.
 */
internal class BlobPayloadRoutingCompatibilityTest {
    @Test
    fun `blob content types preserve single protocol behavior and determine multi protocol routing`() {
        val observations = Files.createTempDirectory("untyped-blob-routing-")
        System.err.println("Untyped blob routing observations: $observations")
        // Generate REST JSON with generated serde, then REST JSON and REST JSON + CBOR with schema serde.
        // All use HTTP 1.x and send identical requests. Generated serde serves one protocol.
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
                    additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
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
                            // Arbitrary, default Blob, and absent Content-Type, with the same raw bytes.
                            for content_type in [#{Some}("video/mp4"), #{Some}("application/octet-stream"), #{None}] {
                                let mut request = #{Http}::Request::post("/upload");
                                if let #{Some}(value) = content_type { request = request.header("content-type", value); }
                                let request = request.body(#{Server}::body::to_boxed_sync("abc")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                // REST-only servers accept all three cases. With CBOR also enabled,
                                // only application/octet-stream identifies REST JSON for this payload.
                                let expected_status = if $multiProtocol && content_type != #{Some}("application/octet-stream") { 404 } else { 200 };
                                assert_eq!(status, expected_status, "{content_type:?}");
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(key, value)| (key.to_string(), value.as_bytes().to_vec())).collect();
                                headers.sort();
                                #{Collect}
                                if status == 200 {
                                    assert_eq!(body.as_ref(), b"{\"size\":3}");
                                } else {
                                    // No protocol claimed the request: pin the shared fallback's
                                    // headers and body as well as its 404 status.
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
        // Compare complete REST-only responses: status, sorted headers, and body bytes.
        // The multi-protocol behavior is asserted separately in its generated Rust test.
        assertEquals(Files.readString(observations.resolve("false-false.txt")), Files.readString(observations.resolve("true-false.txt")))
    }
}
