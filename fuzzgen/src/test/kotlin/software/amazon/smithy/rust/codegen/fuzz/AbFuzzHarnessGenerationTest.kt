/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.fuzz

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.condition.EnabledIfEnvironmentVariable
import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import java.nio.file.Files
import java.nio.file.Path

/**
 * Throwaway, env-gated harness generator for A/B fuzzing per AB_FUZZING.md.
 *
 * Env:
 *  - AB_FUZZ_GENERATE=true              enables the test
 *  - AB_FUZZ_SERVICE=<shape id>         service to fuzz
 *  - AB_FUZZ_MODEL=<paths,comma-sep>    optional extra model imports (local smithy files)
 *  - AB_FUZZ_BEFORE=<path>              generated server crate for target "before"
 *  - AB_FUZZ_AFTER=<path>               generated server crate for target "after"
 *  - AB_FUZZ_OUTPUT=<dir>               harness output directory
 */
class AbFuzzHarnessGenerationTest {
    @Test
    @EnabledIfEnvironmentVariable(named = "AB_FUZZ_GENERATE", matches = "true")
    fun generateAbHarness() {
        val service = System.getenv("AB_FUZZ_SERVICE") ?: error("AB_FUZZ_SERVICE not set")
        val before = System.getenv("AB_FUZZ_BEFORE") ?: error("AB_FUZZ_BEFORE not set")
        val after = System.getenv("AB_FUZZ_AFTER") ?: error("AB_FUZZ_AFTER not set")
        val output = Path.of(System.getenv("AB_FUZZ_OUTPUT") ?: error("AB_FUZZ_OUTPUT not set"))
        Files.createDirectories(output)

        val assembler = Model.assembler().discoverModels()
        System.getenv("AB_FUZZ_MODEL")?.takeIf { it.isNotBlank() }?.split(",")?.forEach {
            assembler.addImport(it)
        }
        val model = assembler.assemble().unwrap()

        val targetCrates =
            listOf("before" to before, "after" to after).map { (name, path) ->
                ObjectNode.objectNode()
                    .withMember("relativePath", path)
                    .withMember("name", name)
            }
        val context =
            PluginContext.builder()
                .model(model)
                .fileManifest(FileManifest.create(output))
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
        println("AB harness written to $output")
    }
}
