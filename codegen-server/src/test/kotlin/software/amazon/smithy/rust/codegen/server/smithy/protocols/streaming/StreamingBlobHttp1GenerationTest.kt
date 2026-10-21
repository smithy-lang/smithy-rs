/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.streaming

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
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
        val root = Path.of(System.getenv("SCHEMA_COMPAT_ROOT") ?: "..").toAbsolutePath().normalize()
        val run =
            System.getenv("SCHEMA_COMPAT_RUN")?.let(Path::of)
                ?: Files.createTempDirectory("schema-compat-streaming-")
                    .let { Files.createTempDirectory(it, "streaming-blob-http1-") }
        val model = Files.readString(root.resolve("codegen-server-test/schema-serde-compat/models/streaming-blob-http1.smithy")).asSmithyModel()
        for (service in listOf("RestBlob", "CborBlob")) {
            val settings =
                ServerAdditionalSettings.builder().withHttp1x().schemaSerde(false).toObjectNode().toBuilder()
            System.getenv("SCHEMA_COMPAT_RUNTIME")?.let {
                settings.withMember("runtimeConfig", Node.objectNodeBuilder().withMember("relativePath", it).build())
            }
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
                                    data: ${if (service == "CborBlob") "#{Types}::Blob::new(bytes.clone())" else "#{Types}::byte_stream::ByteStream::from(bytes.clone())"},
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
