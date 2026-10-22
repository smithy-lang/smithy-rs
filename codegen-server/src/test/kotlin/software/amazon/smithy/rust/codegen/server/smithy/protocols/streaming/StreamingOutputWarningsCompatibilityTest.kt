/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.streaming

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.runWithWarnings
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Generates a required streaming-blob output for legacy and schema HTTP 1.x servers and compiles
 * with warnings denied. It sends a request, checks the payload bytes, and compares status and
 * headers between serde modes. This catches invalid expression wrapping in generated Rust as
 * well as changes to the streaming response's HTTP framing.
 */
internal class StreamingOutputWarningsCompatibilityTest {
    @Test
    fun `streaming blob outputs compile with warnings denied and preserve legacy responses`() {
        val run = Files.createTempDirectory("streaming-output-warnings-parity-")
        val model =
            """
            ${'$'}version: "2"
            namespace test.streamingwarnings
            use aws.protocols#restJson1
            @restJson1 service Streams { version: "1", operations: [Required] }
            @http(method: "POST", uri: "/required")
            operation Required { input := {}, output := { @required @httpPayload body: Stream } }
            @streaming blob Stream
            """.asSmithyModel()
        for (schemaSerde in listOf(false, true)) {
            val snapshot = run.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
                    overrideTestDir = run.resolve(if (schemaSerde) "schema" else "legacy").toFile(),
                    command = { path -> "cargo test --quiet --lib".runWithWarnings(path) },
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                crate.testModule {
                    tokioTest("streaming_blob_output_warnings") {
                        rustTemplate(
                            """
                            let app = crate::Streams::builder(crate::StreamsConfig::builder().build())
                                .required(|_: crate::input::RequiredInput| async move {
                                    crate::output::RequiredOutput {
                                        body: #{Types}::byte_stream::ByteStream::from_static(b"payload"),
                                    }
                                }).build().unwrap();
                            let mut records = #{String}::new();
                            for path in ["/required"] {
                                let request = #{Http}::Request::post(path).header("content-type", "application/json")
                                    .body(#{Server}::body::to_boxed_sync("{}")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string())).collect();
                                headers.sort();
                                #{Collect}
                                assert_eq!(status, 200);
                                assert_eq!(body.as_ref(), b"payload");
                                records.push_str(&format!("{path} {status} {headers:?} {}\n", ::std::str::from_utf8(&body).unwrap()));
                            }
                            ::std::fs::write(${snapshot.toString().dq()}, records).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                        )
                    }
                }
            }
        }
        assertEquals(Files.readString(run.resolve("false.txt")), Files.readString(run.resolve("true.txt")), run.toString())
    }
}
