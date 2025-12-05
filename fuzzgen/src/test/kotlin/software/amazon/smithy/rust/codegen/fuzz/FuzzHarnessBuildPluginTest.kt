/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.fuzz

import io.kotest.matchers.collections.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.printGeneratedFiles
import software.amazon.smithy.rust.codegen.core.util.runCommand
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class FuzzHarnessBuildPluginTest() {
    private val minimalModel =
        """
        namespace com.example
        use aws.protocols#awsJson1_0
        @awsJson1_0
        service HelloService {
            operations: [SayHello],
            version: "1"
        }
        operation SayHello { input: TestInput }
        structure TestInput {
           foo: String,
        }
        """.asSmithyModel()

    /**
     * Smoke test that generates a lexicon and target crate for the trivial service above
     */
    @Test
    fun smokeTest() {
        val testDir = TestWorkspace.subproject()
        val testPath = testDir.toPath()
        val manifest = FileManifest.create(testPath)
        val service = "com.example#HelloService"
        // Only generate server for http@1 as `aws-smithy-fuzz` only supports http@1.
        val generatedServers =
            serverIntegrationTest(
                minimalModel,
                IntegrationTestParams(service = service, command = { dir -> println("generated $dir") }),
                testCoverage =
                    HttpTestType.Only(
                        HttpTestVersion.HTTP_1_X,
                    ),
            ) { _, _ ->
            }

        // Create target crates for each generated server (only HTTP 1.x for fuzz testing)
        val targetCrates =
            generatedServers.map { server ->
                ObjectNode.objectNode()
                    .withMember("relativePath", server.path.toString())
                    .withMember("name", "a")
            }

        val context =
            PluginContext.builder()
                .model(minimalModel)
                .fileManifest(manifest)
                .settings(
                    ObjectNode.objectNode()
                        .withMember("service", "com.example#HelloService")
                        .withMember(
                            "targetCrates",
                            ArrayNode.fromNodes(targetCrates),
                        )
                        .withMember(
                            "runtimeConfig",
                            Node.objectNode().withMember(
                                "relativePath",
                                Node.from(((TestRuntimeConfig).runtimeCrateLocation).path),
                            ),
                        ),
                ).build()
        FuzzHarnessBuildPlugin().execute(context)
        context.fileManifest.printGeneratedFiles()
        context.fileManifest.files.map { it.fileName.toString() } shouldContain "lexicon.json"
        "cargo check".runCommand(context.fileManifest.baseDir.resolve("a"))
    }
}
