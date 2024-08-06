/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.fuzz

import org.junit.jupiter.api.Test
import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.printGeneratedFiles
import java.io.File

class FuzzHarnessBuildPluginTest() {
    fun actualServerCodegenPath(
        model: String,
        extra: String,
    ) =
        File("../../smithy-rs$extra/codegen-server-test/build/smithyprojections/codegen-server-test/$model/rust-server-codegen").absolutePath

    @Test
    fun works() {
        val model = "namespace empty".asSmithyModel()
        val testDir = TestWorkspace.subproject()
        val testPath = testDir.toPath()
        val manifest = FileManifest.create(testPath)
        val modelName = "rest_json"
        val context =
            PluginContext.builder()
                .model(model)
                .fileManifest(manifest)
                .settings(
                    ObjectNode.objectNode()
                        .withMember("service", "aws.protocoltests.restjson#RestJson")
                        .withMember(
                            "targetCrates",
                            ArrayNode.arrayNode(
                                ObjectNode.objectNode().withMember("relativePath", actualServerCodegenPath(modelName, "2"))
                                    .withMember("name", "a"),
                                ObjectNode.objectNode().withMember("relativePath", actualServerCodegenPath(modelName, "3"))
                                    .withMember("name", "b"),
                            ),
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
        print("cd ${context.fileManifest.baseDir.resolve("driver")} && cargo check")
        // "cargo check".runCommand(context.fileManifest.baseDir.resolve("a"))
    }
}
