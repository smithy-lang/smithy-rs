/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.fuzz

import io.kotest.matchers.collections.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
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

class FuzzHarnessBuildPluginTest {
    private data class ProtocolCase(
        val id: String,
        val crateName: String,
        val service: String,
        val model: Model,
    )

    private val protocolCases =
        listOf(
            ProtocolCase(
                id = "rpcv2Cbor",
                crateName = "rpcv2_cbor",
                service = "com.example#RpcV2CborService",
                model =
                    """
                    namespace com.example
                    use smithy.protocols#rpcv2Cbor
                    @rpcv2Cbor
                    service RpcV2CborService {
                        operations: [SayHello],
                        version: "1"
                    }
                    operation SayHello { input: TestInput }
                    structure TestInput {
                       foo: String,
                    }
                    """.asSmithyModel(),
            ),
            ProtocolCase(
                id = "restJson1",
                crateName = "rest_json_1",
                service = "com.example#RestJsonService",
                model =
                    """
                    namespace com.example
                    use aws.protocols#restJson1
                    use smithy.api#http
                    @restJson1
                    service RestJsonService {
                        operations: [SayHello],
                        version: "1"
                    }
                    @http(method: "POST", uri: "/hello", code: 200)
                    operation SayHello { input: TestInput, output: TestOutput }
                    structure TestInput {
                       foo: String,
                    }
                    structure TestOutput {
                       message: String,
                    }
                    """.asSmithyModel(),
            ),
        )

    private fun selectedProtocolCases(): List<ProtocolCase> {
        val selectedProtocol = System.getProperty("smithy.fuzz.protocol", "all")
        return when (selectedProtocol) {
            "all" -> protocolCases
            else -> {
                val protocolCase = protocolCases.find { it.id == selectedProtocol }
                requireNotNull(protocolCase) {
                    "Unknown smithy.fuzz.protocol=$selectedProtocol. Expected one of: all, ${
                        protocolCases.joinToString { it.id }
                    }"
                }
                listOf(protocolCase)
            }
        }
    }

    /**
     * Smoke test that generates a lexicon and target crate for the trivial services above.
     *
     * Use `-Dsmithy.fuzz.protocol=rpcv2Cbor` or `-Dsmithy.fuzz.protocol=restJson1` for a focused run.
     */
    @Test
    fun smokeTest() {
        selectedProtocolCases().forEach { protocolCase ->
            smokeTest(protocolCase)
        }
    }

    private fun smokeTest(protocolCase: ProtocolCase) {
        val testDir = TestWorkspace.subproject()
        val testPath = testDir.toPath()
        val manifest = FileManifest.create(testPath)
        // Only generate server for http@1 as `aws-smithy-fuzz` only supports http@1.
        val generatedServers =
            serverIntegrationTest(
                protocolCase.model,
                IntegrationTestParams(service = protocolCase.service, command = { dir -> println("generated $dir") }),
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
                    .withMember("name", protocolCase.crateName)
            }

        val context =
            PluginContext.builder()
                .model(protocolCase.model)
                .fileManifest(manifest)
                .settings(
                    ObjectNode.objectNode()
                        .withMember("service", protocolCase.service)
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
        "cargo check".runCommand(context.fileManifest.baseDir.resolve(protocolCase.crateName))
    }
}
