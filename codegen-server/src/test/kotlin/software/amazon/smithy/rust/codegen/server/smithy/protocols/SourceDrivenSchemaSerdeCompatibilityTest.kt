/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertTrue
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

/** Default CI runs a small wire-parity corpus; the external runner selects the complete audit. */
internal class SourceDrivenSchemaSerdeCompatibilityTest {
    private val externalAudit = System.getenv("SCHEMA_COMPAT_ROOT") != null
    private val root = Path.of(System.getenv("SCHEMA_COMPAT_ROOT") ?: "..").toAbsolutePath().normalize()
    private val run = System.getenv("SCHEMA_COMPAT_RUN")?.let(Path::of) ?: Files.createTempDirectory("schema-compat-ci-")
    private val protocols =
        System.getenv("SCHEMA_COMPAT_PROTOCOLS")?.split(",")
            ?: listOf("rest-json", "rest-xml", "aws-json-10", "aws-json-11", "rpc-v2-cbor")

    private fun settings(schemaSerde: Boolean): ObjectNode =
        Node.objectNodeBuilder()
            .withMember("codegen", Node.objectNodeBuilder().withMember("http-1x", true).withMember("schemaSerde", schemaSerde).build())
            .withMember("runtimeConfig", Node.objectNodeBuilder().withMember("relativePath", (System.getenv("SCHEMA_COMPAT_RUNTIME") ?: root.resolve("rust-runtime").toString())).build())
            .build()

    @Test
    fun `generate fresh legacy and schema services and observe identical requests`() {
        val suite = root.resolve("tools/schema-serde-compat")
        if (!externalAudit) {
            Files.createDirectories(run.resolve("cases"))
            val essential =
                setOf(
                    "body-object", "body-known-null", "body-nested-valid", "body-nested-missing",
                    "body-blob-good", "query-empty", "xml-root", "xml-empty", "response-empty", "response-zero", "response-quotes",
                )
            for (protocol in protocols) {
                val cases =
                    Node.parse(Files.readString(suite.resolve("cases/$protocol.json"))).expectArrayNode().elements
                        .filter { it.expectObjectNode().expectStringMember("id").value in essential }
                assertTrue(cases.isNotEmpty(), protocol)
                Files.writeString(run.resolve("cases/$protocol.json"), Node.printJson(Node.arrayNode(*cases.toTypedArray())))
            }
        }
        for (protocol in protocols) {
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
            if (!externalAudit) {
                fun wire(mode: String) =
                    Files.readAllLines(run.resolve("$protocol-$mode.jsonl")).map { line ->
                        val observation = Node.parse(line).expectObjectNode()
                        assertTrue(observation.expectMember("panic").isNullNode, "$protocol $mode: $observation")
                        observation.withoutMember("logs")
                    }
                assertEquals(wire("legacy"), wire("schema"), "$protocol: $run")
            }
        }
    }

    @Test
    fun `record sensitive model compilation on both generators`() {
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
                            val status = process.start().waitFor()
                            results.add("$probe\t$mode\t$status")
                            if (!externalAudit) {
                                // Fresh mainline fails these probes under the same warning policy.
                                // Assert the observed failure, and require the schema generator to compile.
                                val knownLegacyFailure = !schemaSerde && probe != "sensitive-query-map"
                                assertEquals(if (knownLegacyFailure) 101 else 0, status, "$probe/$mode: ${run.resolve("compile-$probe-$mode.log")}")
                                if (knownLegacyFailure) {
                                    val diagnostic =
                                        when (probe) {
                                            "sensitive-prefix" -> "Sensitive Data Redacted"
                                            "sensitive-query-map" -> "unused variable: `name`"
                                            else -> "method `set_token` is never used"
                                        }
                                    assertTrue(Files.readString(run.resolve("compile-$probe-$mode.log")).contains(diagnostic), diagnostic)
                                }
                            }
                        },
                    ),
                    testCoverage = HttpTestType.Default,
                )
            }
        }
        Files.write(run.resolve("compile-probes.tsv"), results)
    }
}
