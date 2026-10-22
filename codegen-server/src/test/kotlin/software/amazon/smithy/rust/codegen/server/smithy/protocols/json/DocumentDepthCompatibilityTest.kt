/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.json

import org.junit.jupiter.api.Assumptions.assumeTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

/**
 * Optional audit that generates legacy and schema servers for deeply nested JSON documents under
 * REST JSON and AWS JSON 1.0/1.1. It runs the supplied observation harness and writes per-protocol
 * captures for external comparison; this class does not itself assert legacy/schema equality.
 * Run it with SCHEMA_COMPAT_C10=1 and a SCHEMA_COMPAT_RUN directory containing the required inputs
 * and request corpora, plus SCHEMA_COMPAT_RUNTIME for the runtime checkout.
 */
internal class DocumentDepthCompatibilityTest {
    @Test
    fun `generate fresh document depth observations`() {
        assumeTrue(System.getenv("SCHEMA_COMPAT_C10") == "1", "Set SCHEMA_COMPAT_C10=1 to run the document-depth audit")
        val run = Path.of(System.getenv("SCHEMA_COMPAT_RUN"))
        val inputs = run.resolve("inputs")
        val model = Files.readString(inputs.resolve("document-depth.smithy")).asSmithyModel()
        for ((protocol, service) in listOf("rest-json" to "RestDepth", "aws-json-10" to "Json10Depth", "aws-json-11" to "Json11Depth")) {
            for (schemaSerde in listOf(false, true)) {
                val mode = if (schemaSerde) "schema" else "legacy"
                val settings =
                    ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode().toBuilder()
                        .withMember("runtimeConfig", Node.objectNodeBuilder().withMember("relativePath", System.getenv("SCHEMA_COMPAT_RUNTIME")).build())
                        .build()
                Files.writeString(run.resolve("$protocol-$mode-settings.json"), Node.prettyPrintJson(settings))
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        service = "compat.documentdepth#$service",
                        overrideTestDir = run.resolve("$protocol/$mode").toFile(),
                        additionalSettings = settings,
                        command = { path ->
                            val log = run.resolve("$protocol-$mode-cargo.log")
                            val process =
                                ProcessBuilder("cargo", "test", "--quiet", "--lib", "compat_observe", "--", "--nocapture")
                                    .directory(path.toFile()).redirectErrorStream(true).redirectOutput(log.toFile())
                            process.environment()["RUSTFLAGS"] = "-D warnings"
                            process.environment()["SCHEMA_COMPAT_OBSERVATIONS"] = run.resolve("$protocol-$mode.jsonl").toString()
                            check(process.start().waitFor() == 0) { "Cargo failed: $log" }
                        },
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    check(context.usesSchemaHttpSerde == schemaSerde)
                    crate.testModule {
                        rustTemplate(
                            Files.readString(inputs.resolve("observe-c10.rs.in")),
                            "Service" to RuntimeType("crate::$service"),
                            "Config" to RuntimeType("crate::${service}Config"),
                            "PayloadHandler" to
                                writable {
                                    if (protocol == "rest-json") {
                                        writeWithNoFormatting(
                                            ".document_payload(|input: crate::input::DocumentPayloadInput| async move { record(&input.document); crate::output::DocumentPayloadOutput::builder().build() })",
                                        )
                                    }
                                },
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Json" to CargoDependency.SerdeJson.toType(),
                            "Tokio" to CargoDependency.Tokio.toType(),
                            "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                            "Futures" to CargoDependency.FuturesUtil.copy(features = setOf("std", "async-await")).toType(),
                            "CorpusPath" to run.resolve("cases/$protocol.json").toString(),
                            "BodyToBytes" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "bytes", "response"),
                        )
                    }
                }
            }
        }
    }
}
