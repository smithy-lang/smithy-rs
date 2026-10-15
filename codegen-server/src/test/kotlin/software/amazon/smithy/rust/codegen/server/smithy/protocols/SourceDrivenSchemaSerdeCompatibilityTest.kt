/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Assumptions.assumeTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

/** Invoked by tools/schema-serde-compat/run.py; observations are compared by that runner. */
internal class SourceDrivenSchemaSerdeCompatibilityTest {
    private fun settings(schemaSerde: Boolean): ObjectNode =
        Node.objectNodeBuilder()
            .withMember("codegen", Node.objectNodeBuilder().withMember("http-1x", true).withMember("schemaSerde", schemaSerde).build())
            .withMember("runtimeConfig", Node.objectNodeBuilder().withMember("relativePath", System.getenv("SCHEMA_COMPAT_RUNTIME")).build())
            .build()

    @Test
    fun `generate fresh legacy and schema services and observe identical requests`() {
        val rootEnv = System.getenv("SCHEMA_COMPAT_ROOT")
        assumeTrue(rootEnv != null, "Run tools/schema-serde-compat/run.py to select and record the audit")
        val root = Path.of(rootEnv)
        val suite = root.resolve("tools/schema-serde-compat")
        val run = Path.of(System.getenv("SCHEMA_COMPAT_RUN"))
        for (protocol in System.getenv("SCHEMA_COMPAT_PROTOCOLS").split(",")) {
            val commonFile = if (protocol == "rpc-v2-cbor") "cbor-shapes.smithy" else "bindings-and-json.smithy"
            val common = Files.readString(suite.resolve("models/$commonFile")).asSmithyModel()
            val model = Model.assembler().addModel(common).addImport(suite.resolve("models/$protocol.smithy")).assemble().unwrap()
            for (schemaSerde in listOf(false, true)) {
                val mode = if (schemaSerde) "schema" else "legacy"
                val generated = run.resolve("$protocol/$mode")
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        service = "compat.audit#Audit",
                        overrideTestDir = generated.toFile(),
                        additionalSettings = settings(schemaSerde),
                        command = { path ->
                            val process =
                                ProcessBuilder("cargo", "test", "--quiet", "--lib", "compat_observe", "--", "--nocapture")
                                    .directory(path.toFile())
                                    .redirectErrorStream(true)
                                    .redirectOutput(run.resolve("$protocol-$mode-cargo.log").toFile())
                            process.environment()["SCHEMA_COMPAT_OBSERVATIONS"] = run.resolve("$protocol-$mode.jsonl").toString()
                            // Keep the strict compilation probe separate from runtime observations:
                            // legacy XML attributes leave a generated setter unused.
                            process.environment()["RUSTFLAGS"] = if (protocol == "rest-xml") "-D warnings -A dead_code" else "-D warnings"
                            check(process.start().waitFor() == 0) {
                                "Cargo failed: ${run.resolve("$protocol-$mode-cargo.log")}"
                            }
                        },
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    val handlers =
                        context.serviceShape.allOperations.map {
                            context.model.expectShape(it, OperationShape::class.java)
                        }.sortedBy { it.id.name }.joinToString("\n") { op: OperationShape ->
                            val name = op.id.name
                            val record =
                                when (name) {
                                    "Bindings" -> "record_bindings(&input);"
                                    "Body" -> "record_body(&input);"
                                    "XmlBody" -> "record_xml(&input);"
                                    "Stream" -> "record_events(input).await;"
                                    "StreamingBlob" -> "record_streaming_blob(input).await;"
                                    else -> "record_debug(&input);"
                                }
                            val output =
                                when (name) {
                                    "Response" -> "response_output(input)"
                                    "MediaResponse" -> "crate::output::MediaResponseOutput::builder().meta(Some(std::collections::HashMap::from([(\"key\".into(), \"{}\".into())]))).build()"
                                    "Sensitive" -> "crate::output::SensitiveOutput::builder().secret(Some(\"SECRET_RESPONSE\".into())).build()"
                                    else -> "crate::output::${name}Output::builder().build()"
                                }
                            val returned =
                                if (name == "Modeled") {
                                    "Err::<crate::output::ModeledOutput, crate::error::ModeledError>(modeled_error(input).into())"
                                } else if (op.errors.isEmpty()) {
                                    output
                                } else {
                                    "Ok::<_, crate::error::${name}Error>($output)"
                                }
                            ".${name.toSnakeCase()}(|input: crate::input::${name}Input| async move { $record $returned })"
                        }
                    crate.testModule {
                        rustTemplate(
                            Files.readString(suite.resolve("harness.rs.in")),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Json" to CargoDependency.SerdeJson.toType(),
                            "Tokio" to CargoDependency.Tokio.toType(),
                            "Tracing" to CargoDependency.Tracing.toType(),
                            "Subscriber" to CargoDependency.TracingSubscriber.toType(),
                            "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                            "Futures" to CargoDependency.FuturesUtil.copy(features = setOf("std", "async-await")).toType(),
                            "CorpusPath" to run.resolve("cases/$protocol.json").toString(),
                            "Handlers" to writable { writeWithNoFormatting(handlers) },
                            "BodyToBytes" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "bytes", "response"),
                            "ObserveBody" to
                                writable {
                                    val file =
                                        when (protocol) {
                                            "rest-xml" -> "observe-xml.rs.in"
                                            "rpc-v2-cbor" -> "observe-cbor.rs.in"
                                            else -> "observe-json.rs.in"
                                        }
                                    writeWithNoFormatting(Files.readString(suite.resolve(file)))
                                },
                            "ObserveStreaming" to
                                writable {
                                    if (protocol in listOf("rest-json", "rpc-v2-cbor")) {
                                        val bytes =
                                            if (protocol == "rpc-v2-cbor" && !context.usesSchemaHttpSerde) {
                                                "input.value.as_ref().to_vec()"
                                            } else {
                                                "input.value.collect().await.unwrap().into_bytes()"
                                            }
                                        writeWithNoFormatting(Files.readString(suite.resolve("observe-streaming.rs.in")).replace("__BLOB_BYTES__", bytes))
                                    }
                                },
                        )
                    }
                }
            }
        }
    }

    @Test
    fun `record sensitive model compilation on both generators`() {
        assumeTrue(System.getenv("SCHEMA_COMPAT_ROOT") != null)
        val root = Path.of(System.getenv("SCHEMA_COMPAT_ROOT"))
        val run = Path.of(System.getenv("SCHEMA_COMPAT_RUN"))
        val results = mutableListOf<String>()
        for (probe in listOf("sensitive-prefix", "sensitive-query-map", "xml-attribute")) {
            val model = Files.readString(root.resolve("tools/schema-serde-compat/models/$probe.smithy")).asSmithyModel()
            for (schemaSerde in listOf(false, true)) {
                val mode = if (schemaSerde) "schema" else "legacy"
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        service = "compat.compileprobe#Audit",
                        overrideTestDir = run.resolve("compile-$probe/$mode").toFile(),
                        additionalSettings = settings(schemaSerde),
                        command = { path ->
                            val process =
                                ProcessBuilder("cargo", "check", "--quiet", "--lib").directory(path.toFile())
                                    .redirectErrorStream(true).redirectOutput(run.resolve("compile-$probe-$mode.log").toFile())
                            process.environment()["RUSTFLAGS"] = "-D warnings"
                            results.add("$probe\t$mode\t${process.start().waitFor()}")
                        },
                    ),
                    testCoverage = HttpTestType.Default,
                )
            }
        }
        Files.write(run.resolve("compile-probes.tsv"), results)
    }
}
