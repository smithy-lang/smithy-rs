/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.fuzz

import io.kotest.matchers.collections.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.condition.EnabledIfEnvironmentVariable
import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File

/**
 * Local-only harness generator: produces an RPC v2 CBOR server plus an `aws-smithy-fuzz`
 * target crate so the generated routing/serde code can be fuzzed with `cargo afl`.
 *
 * The generated server uses the working tree's `rust-runtime`, so it exercises the current
 * `aws-smithy-http-server` RPC v2 CBOR router.
 *
 * Only HTTP 1.x is generated: `aws-smithy-fuzz` is built on `http` 1.x / `http-body` 1.0.
 */
class RpcV2CborFuzzHarnessGenerationTest {
    private val service = "smithy.protocoltests.rpcv2Cbor#RpcV2Protocol"

    @Test
    @EnabledIfEnvironmentVariable(named = "RPCV2_FUZZ_GENERATE", matches = "true")
    fun generateRpcV2CborFuzzHarness() {
        val outputDir = File(System.getenv("RPCV2_FUZZ_OUTPUT") ?: "/tmp/rpcv2-cbor-fuzz")
        val serverDir = outputDir.resolve("server-http-1x")
        val harnessDir = outputDir.resolve("harness")
        val beforeServer = System.getenv("RPCV2_FUZZ_BEFORE_SERVER")?.let(::File)
        val afterServer = System.getenv("RPCV2_FUZZ_AFTER_SERVER")?.let(::File)
        val isDifferentialHarness = beforeServer != null && afterServer != null

        listOfNotNull(if (isDifferentialHarness) null else serverDir, harnessDir).forEach {
            it.deleteRecursively()
            it.mkdirs()
        }

        // The rpcv2Cbor protocol tests ship in the `smithy-protocol-tests` artifact.
        val model = Model.assembler().discoverModels(javaClass.classLoader).assemble().unwrap()

        val targetCrates =
            if (isDifferentialHarness) {
                listOf(
                    ObjectNode.objectNode()
                        .withMember("relativePath", beforeServer!!.absolutePath)
                        .withMember("name", "before"),
                    ObjectNode.objectNode()
                        .withMember("relativePath", afterServer!!.absolutePath)
                        .withMember("name", "after"),
                )
            } else {
                val generatedServers =
                    serverIntegrationTest(
                        model,
                        IntegrationTestParams(
                            service = service,
                            overrideTestDir = serverDir,
                            // Do not compile/test here; the fuzz target build compiles everything.
                            command = { dir -> println("generated server: $dir") },
                        ),
                        testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
                    ) { _, _ -> }

                generatedServers.map { server ->
                    ObjectNode.objectNode()
                        .withMember("relativePath", server.path.toString())
                        .withMember("name", "current")
                }
            }

        val manifest = FileManifest.create(harnessDir.toPath())
        val context =
            PluginContext.builder()
                .model(model)
                .fileManifest(manifest)
                .settings(
                    ObjectNode.objectNode()
                        .withMember("service", service)
                        .withMember("targetCrates", ArrayNode.fromNodes(targetCrates))
                        .withMember(
                            "runtimeConfig",
                            Node.objectNode().withMember(
                                "relativePath",
                                Node.from(TestRuntimeConfig.runtimeCrateLocation.path),
                            ),
                        ),
                ).build()

        FuzzHarnessBuildPlugin().execute(context)

        context.fileManifest.files.map { it.fileName.toString() } shouldContain "lexicon.json"

        if (isDifferentialHarness) {
            println("FUZZ_BEFORE_HARNESS_DIR=${harnessDir.resolve("before").absolutePath}")
            println("FUZZ_AFTER_HARNESS_DIR=${harnessDir.resolve("after").absolutePath}")
        } else {
            println("FUZZ_HARNESS_DIR=${harnessDir.resolve("current").absolutePath}")
        }
        println("FUZZ_LEXICON=${harnessDir.resolve("lexicon.json").absolutePath}")
    }
}
