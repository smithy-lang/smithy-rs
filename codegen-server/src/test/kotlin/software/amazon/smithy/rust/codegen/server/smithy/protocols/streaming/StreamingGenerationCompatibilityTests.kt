/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.streaming

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
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
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

/**
 * Generates HTTP 1.x servers with schemaSerde=false from the streaming-blob fixture for REST
 * and CBOR. It compiles and exercises the generated blob handlers, checking successful status
 * and expected response bytes. This protects the legacy streaming code path after adopting the
 * HTTP 1.x body types.
 */
internal class StreamingBlobHttp1GenerationTest {
    @Test
    fun `generate streaming blob servers with HTTP 1 and generated serde`() {
        val root = Path.of("..").toAbsolutePath().normalize()
        val run =
            Files.createTempDirectory("schema-compat-streaming-")
                .let { Files.createTempDirectory(it, "streaming-blob-http1-") }
        val model = Files.readString(root.resolve("codegen-server/src/test/resources/protocol-compat/models/streaming-blob-http1.smithy")).asSmithyModel()
        for (service in listOf("RestBlob", "CborBlob")) {
            val settings =
                ServerAdditionalSettings.builder().withHttp1x().schemaSerde(false).toObjectNode().toBuilder()
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    service = "compat.streamingblob#$service",
                    additionalSettings = settings.build(),
                    overrideTestDir = run.resolve(service.lowercase()).toFile(),
                    command = { path ->
                        val manifest = Files.readString(path.resolve("Cargo.toml"))
                        check(!manifest.contains("aws-smithy-legacy-http-server"))
                        check(manifest.contains("aws-smithy-http-server"))
                        val operation = Files.readString(path.resolve("src/operation.rs"))
                        check(operation.contains("FromRequest<"))
                        check(operation.contains("IntoResponse<"))
                        val log = run.resolve("$service-cargo.log")
                        val process =
                            ProcessBuilder("cargo", "test", "--quiet", "--lib")
                                .directory(path.toFile()).redirectErrorStream(true).redirectOutput(log.toFile())
                        check(process.start().waitFor() == 0) { "Cargo failed: $log" }
                    },
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                check(!context.usesSchemaHttpSerde)
                crate.testModule {
                    tokioTest("capture_legacy_blob_responses") {
                        val checkBody =
                            if (service == "CborBlob") {
                                """
                                let mut expected = vec![0xbf, 0x64, b'd', b'a', b't', b'a', 0x40 + bytes.len() as u8];
                                expected.extend_from_slice(&bytes);
                                expected.push(0xff);
                                assert_eq!(body.as_ref(), expected);
                                """
                            } else {
                                "assert_eq!(body.as_ref(), bytes);"
                            }
                        rustTemplate(
                            """
                            let mut records = #{String}::new();
                            for bytes in [b"hello".to_vec(), vec![], vec![0, 255, 1]] {
                                let output = crate::output::BlobEchoOutput {
                                    data: ${if (service == "CborBlob") {
                                "#{Types}::Blob::new(bytes.clone())"
                            } else {
                                "#{Types}::byte_stream::ByteStream::from(bytes.clone())"
                            }},
                                };
                                let response = crate::protocol_serde::shape_blob_echo::ser_blob_echo_http_response(output).unwrap();
                                let status = response.status();
                                let headers = format!("{:?}", response.headers());
                                let body = #{BodyUtil}::BodyExt::collect(#{BodyUtil}::Limited::new(response.into_body(), 4096)).await.unwrap().to_bytes();
                                $checkBody
                                assert_eq!(status.as_u16(), 200);
                                records.push_str(&format!("{status}\t{headers}\t{:02x?}\n", body.as_ref()));
                            }
                            ::std::fs::write("${run.resolve("$service-baseline.txt")}", records).unwrap();
                            """,
                            "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            *RuntimeType.preludeScope,
                        )
                    }
                }
            }
        }
    }
}

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
                    overrideTestDir =
                        run.resolve(
                            if (schemaSerde) {
                                "schema"
                            } else {
                                "legacy"
                            },
                        ).toFile(),
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
