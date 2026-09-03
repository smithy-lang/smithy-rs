/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.fuzz

import io.kotest.matchers.collections.shouldContain
import org.junit.jupiter.api.Assumptions.assumeTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AbstractTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.printGeneratedFiles
import software.amazon.smithy.rust.codegen.core.util.runCommand
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File
import java.nio.file.Files
import java.nio.file.Path

class FuzzHarnessBuildPluginTest() {
    private enum class PokemonProtocol(
        val outputName: String,
        val trait: AbstractTrait,
    ) {
        AwsJson10("aws-json-10", AwsJson1_0Trait.builder().build()),
        AwsJson11("aws-json-11", AwsJson1_1Trait.builder().build()),
        RestJson1("rest-json1", RestJson1Trait.builder().build()),
        RestXml("rest-xml", RestXmlTrait.builder().build()),
        RpcV2Cbor("rpcv2-cbor", Rpcv2CborTrait.builder().build()),
    }

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

    @Test
    fun `generate Pokemon single protocol versus multi protocol fuzz harnesses`() {
        assumeTrue(
            System.getenv("POKEMON_MP_FUZZ_GENERATE") == "true",
            "Set POKEMON_MP_FUZZ_GENERATE=true to generate local Pokemon multi-protocol fuzz harnesses.",
        )

        val outputRoot = Path.of(System.getenv("POKEMON_MP_FUZZ_OUTPUT") ?: "/tmp/smithy-pokemon-mp-fuzz")
        val baselineRoot =
            Path.of(
                requireNotNull(System.getenv("POKEMON_MP_FUZZ_BASELINE_ROOT")) {
                    "Set POKEMON_MP_FUZZ_BASELINE_ROOT to single-protocol server outputs generated by a clean smithy-rs checkout."
                },
            )
        val service = ShapeId.from("com.aws.example#PokemonService")
        val baseModel = loadPokemonModel()
        val multiProtocolModel = baseModel.withProtocolTraits(service, PokemonProtocol.values().map { it.trait })

        PokemonProtocol.values().forEach { protocol ->
            val harnessModel = loadPokemonModel(protocol).withProtocolTraits(service, listOf(protocol.trait))
            val protocolRoot = outputRoot.resolve(protocol.outputName)
            Files.createDirectories(protocolRoot)

            val singleProtocolServer = findBaselineSingleProtocolServer(baselineRoot, protocol)
            val multiProtocolServer =
                generatePokemonServer(
                    model = multiProtocolModel,
                    service = service,
                    outputDirectory = protocolRoot.resolve("multi-server-http-1x"),
                )

            val manifest = FileManifest.create(protocolRoot.resolve("harness"))
            val targetCrates =
                listOf(
                    targetCrateNode("single", singleProtocolServer),
                    targetCrateNode("multi", multiProtocolServer),
                )
            val context =
                PluginContext.builder()
                    .model(harnessModel)
                    .fileManifest(manifest)
                    .settings(
                        ObjectNode.objectNode()
                            .withMember("service", service.toString())
                            .withMember("targetCrates", ArrayNode.fromNodes(targetCrates))
                            .withMember(
                                "runtimeConfig",
                                Node.objectNode().withMember(
                                    "relativePath",
                                    Node.from(((TestRuntimeConfig).runtimeCrateLocation).path),
                                ),
                            ),
                    ).build()

            FuzzHarnessBuildPlugin().execute(context)
            "cargo check".runCommand(context.fileManifest.baseDir.resolve("single"))
            "cargo check".runCommand(context.fileManifest.baseDir.resolve("multi"))
        }
    }

    private fun loadPokemonModel(): Model =
        Model.assembler()
            .discoverModels()
            .addImport(File("../codegen-core/common-test-models/pokemon.smithy").absolutePath)
            .addImport(File("../codegen-core/common-test-models/pokemon-common.smithy").absolutePath)
            .assemble()
            .unwrap()

    private fun loadPokemonModel(protocol: PokemonProtocol): Model =
        when (protocol) {
            PokemonProtocol.AwsJson10, PokemonProtocol.AwsJson11 ->
                Model.assembler()
                    .discoverModels()
                    .addImport(File("../codegen-core/common-test-models/pokemon-awsjson.smithy").absolutePath)
                    .addImport(File("../codegen-core/common-test-models/pokemon-common.smithy").absolutePath)
                    .assemble()
                    .unwrap()
            PokemonProtocol.RestJson1, PokemonProtocol.RestXml, PokemonProtocol.RpcV2Cbor -> loadPokemonModel()
        }

    private fun findBaselineSingleProtocolServer(
        baselineRoot: Path,
        protocol: PokemonProtocol,
    ): Path {
        val candidates =
            listOf(
                baselineRoot.resolve(protocol.outputName).resolve("server-http-1x"),
                baselineRoot.resolve(protocol.outputName).resolve("single-server-http-1x"),
                baselineRoot.resolve(protocol.outputName),
            )
        return candidates.firstOrNull { Files.isRegularFile(it.resolve("Cargo.toml")) }
            ?: error(
                "No clean single-protocol server Cargo.toml found for ${protocol.outputName}. " +
                    "Checked: ${candidates.joinToString()}",
            )
    }

    private fun Model.withProtocolTraits(
        service: ShapeId,
        traits: List<AbstractTrait>,
    ): Model {
        val serviceBuilder = expectShape(service, ServiceShape::class.java).toBuilder()
        PokemonProtocol.values().forEach { serviceBuilder.removeTrait(it.trait.toShapeId()) }
        traits.forEach { serviceBuilder.addTrait(it) }
        return ModelTransformer.create().replaceShapes(this, listOf(serviceBuilder.build()))
    }

    private fun generatePokemonServer(
        model: Model,
        service: ShapeId,
        outputDirectory: Path,
    ): Path {
        val generatedServers =
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    service = service.toString(),
                    overrideTestDir = outputDirectory.toFile(),
                    command = {},
                    additionalSettings =
                        ServerAdditionalSettings.builder()
                            .withHttp1x()
                            .toObjectNode(),
                ),
                testCoverage = HttpTestType.Default,
            ) { _, _ -> }
        return generatedServers.single().path
    }

    private fun targetCrateNode(
        name: String,
        path: Path,
    ): ObjectNode =
        ObjectNode.objectNode()
            .withMember("relativePath", path.toString())
            .withMember("name", name)
}
