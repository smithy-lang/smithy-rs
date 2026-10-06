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
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File
import java.nio.file.Path

/**
 * Generates one side of the single-protocol versus multi-protocol A/B fuzz harnesses.
 *
 * Run it from a clean `main` checkout with `MP_FUZZ_SIDE=single` to produce a legacy single-protocol server and
 * fuzz target per protocol, and from the multi-protocol branch with `MP_FUZZ_SIDE=multi` to produce one server
 * serving every protocol and its fuzz target. Both sides read the same model files from `MP_FUZZ_MODELS`.
 *
 * Output layout under `MP_FUZZ_OUTPUT`:
 * ```
 * <suite>/<protocol>/single-server/     <suite>/<protocol>/single-harness/{single/,lexicon.json}
 * <suite>/multi-server/                 <suite>/multi-harness/{multi/,lexicon.json}
 * <suite>/multi-xml-server/             <suite>/multi-xml-harness/{multi/,lexicon.json}
 * ```
 *
 * The `single` checkout's fuzz target generator may implement a different set of operations than this one, and
 * an operation only one target implements diverges on every request. So when `MP_FUZZ_SINGLE_RUNTIME` names the
 * `rust-runtime` directory of the checkout that generated the single-protocol servers, the `multi` side also
 * generates both targets of each comparison itself:
 * ```
 * <suite>/<protocol>/ab-harness/{single/,multi/,lexicon.json}       against multi-server
 * <suite>/rest-xml/ab-harness-xml/{single/,multi/,lexicon.json}     against multi-xml-server
 * ```
 */
@EnabledIfEnvironmentVariable(named = "MP_FUZZ_GENERATE", matches = "true")
class MultiProtocolAbFuzzHarnessTest {
    private data class Protocol(val dir: String, val shapeId: String, val multiProtocolFile: String) {
        val trait = shapeId.substringAfter('#')
    }

    private val protocols =
        listOf(
            Protocol("aws-json-10", "aws.protocols#awsJson1_0", "multi-protocol-awsjson10.smithy"),
            Protocol("aws-json-11", "aws.protocols#awsJson1_1", "multi-protocol-awsjson11.smithy"),
            Protocol("rest-json1", "aws.protocols#restJson1", "multi-protocol-restjson.smithy"),
            Protocol("rest-xml", "aws.protocols#restXml", "multi-protocol-restxml.smithy"),
            Protocol("rpcv2-cbor", "smithy.protocols#rpcv2Cbor", "multi-protocol-rpcv2cbor.smithy"),
        )

    private val side = System.getenv("MP_FUZZ_SIDE") ?: error("MP_FUZZ_SIDE must be `single` or `multi`")
    private val output = File(System.getenv("MP_FUZZ_OUTPUT") ?: error("MP_FUZZ_OUTPUT is required"))
    private val models = File(System.getenv("MP_FUZZ_MODELS") ?: error("MP_FUZZ_MODELS is required"))
    private val suites = (System.getenv("MP_FUZZ_SUITES") ?: "pokemon,multiprotocol").split(",")
    private val singleRuntime = System.getenv("MP_FUZZ_SINGLE_RUNTIME")?.let { File(it) }
    private val selected =
        System.getenv("MP_FUZZ_PROTOCOLS")?.split(",")?.let { names -> protocols.filter { it.dir in names } }
            ?: protocols

    private fun assemble(vararg sources: Pair<String, String>): Model =
        Model.assembler().discoverModels().apply {
            sources.forEach { (name, text) -> addUnparsedModel(name, text) }
        }.assemble().unwrap()

    /**
     * The Pokémon service with its `@restJson1` trait replaced by [with].
     *
     * Only the plain blob stream is removed. The capture stream's region is omitted because legacy
     * RPC codegen rejects streaming inputs with additional body members.
     */
    private fun pokemon(with: List<Protocol>): Model {
        val service =
            models.resolve("pokemon.smithy").readText()
                .replace("        StreamPokemonRadio\n", "")
                // Legacy RPC codegen requires the event stream to be the sole input member.
                // Apply the same reduced shape on both sides and on every protocol.
                .replace("/capture-pokemon-event/{region}", "/capture-pokemon-event")
                .replace(Regex("@httpLabel\\s+@required\\s+region: String"), "")
                .replace(
                    "use aws.protocols#restJson1\n",
                    with.joinToString("") { "use ${it.shapeId}\n" },
                )
                .replace("@restJson1\nservice PokemonService", with.joinToString("") { "@${it.trait}\n" } + "service PokemonService")
        check(with.all { service.contains("@${it.trait}\n") }) { "failed to apply protocol traits to pokemon.smithy" }
        return assemble(
            "pokemon.smithy" to service,
            "pokemon-common.smithy" to models.resolve("pokemon-common.smithy").readText(),
        )
    }

    /**
     * The multi-protocol test service bound in [file], without its plain blob streaming operation, as for [pokemon].
     *
     * `Greet`'s `@http(code: 201)` is dropped as well. A legacy awsJson server answers with that status and a
     * schema server with `200`, so every successful awsJson `Greet` would otherwise be a divergence, and the
     * fuzzer would explore no further than the first one.
     */
    private fun multiProtocol(
        file: String,
        edit: (String) -> String = { it },
    ): Model {
        val common =
            models.resolve("multi-protocol-common.smithy").readText()
                .replace(
                    "operation Publish {\n    input := {\n        @required\n        @httpHeader(\"x-topic\")\n        topic: String",
                    "operation Publish {\n    input := {",
                )
        check(common.contains(", code: 201)")) { "expected `Greet` to declare `code: 201`" }
        return assemble(
            file to
                listOf("Upload").fold(edit(models.resolve(file).readText())) { text, op ->
                    text.replace("        $op\n", "")
                },
            "multi-protocol-common.smithy" to common.replace(", code: 201)", ")"),
        ).let { model ->
            // serverIntegrationTest adds validation errors for constrained inputs. Include them
            // explicitly so the fuzz target sees the same handler return types as the server.
            ModelTransformer.create().mapShapes(model) { shape ->
                if (shape is OperationShape && shape.id.name in listOf("Subscribe", "Publish")) {
                    shape.toBuilder().addErrors(listOf(ShapeId.from("smithy.framework#ValidationException"))).build()
                } else {
                    shape
                }
            }
        }
    }

    private fun serviceOf(suite: String) =
        when (suite) {
            "pokemon" -> "com.aws.example#PokemonService"
            "multiprotocol" -> "com.example.multiprotocol#MultiProtocolService"
            else -> error("unknown suite $suite")
        }

    private fun singleModel(
        suite: String,
        protocol: Protocol,
    ) = if (suite == "pokemon") pokemon(listOf(protocol)) else multiProtocol(protocol.multiProtocolFile)

    private fun multiModel(
        suite: String,
        served: List<Protocol>,
    ): Model =
        if (suite == "pokemon") {
            pokemon(served)
        } else {
            multiProtocol("multi-protocol.smithy") { text ->
                protocols.filterNot { it in served }.fold(text) { acc, p -> acc.replace("@${p.trait}\n", "") }
            }
        }

    /** Generates an HTTP 1.x server for [model] and copies the crate to [dest]. */
    private fun generateServer(
        model: Model,
        service: String,
        dest: File,
    ): File {
        val codegen = Node.objectNodeBuilder().withMember("http-1x", true)
        codegen.withMember("schemaSerde", side == "multi")
        val servers =
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    service = service,
                    additionalSettings = Node.objectNode().withMember("codegen", codegen.build()),
                    command = { dir -> println("generated $dir") },
                ),
                testCoverage = HttpTestType.Default,
            )
        dest.deleteRecursively()
        servers.single().path.toFile().copyRecursively(dest)
        dest.resolve("target").deleteRecursively()
        return dest
    }

    /** Generates the fuzz target [name] for [server] under [dest], alongside any targets already there. */
    private fun generateFuzzTarget(
        model: Model,
        service: String,
        server: File,
        dest: File,
        name: String = side,
        runtime: File = File(TestRuntimeConfig.runtimeCrateLocation.path!!),
    ) {
        val context =
            PluginContext.builder()
                .model(model)
                .fileManifest(FileManifest.create(dest.toPath()))
                .settings(
                    ObjectNode.objectNode()
                        .withMember("service", service)
                        .withMember(
                            "targetCrates",
                            ArrayNode.fromNodes(
                                listOf(
                                    ObjectNode.objectNode()
                                        .withMember("relativePath", server.absolutePath)
                                        .withMember("name", name),
                                ),
                            ),
                        )
                        .withMember(
                            "runtimeConfig",
                            Node.objectNode().withMember(
                                "relativePath",
                                Node.from(Path.of(runtime.path).toAbsolutePath().toString()),
                            ),
                        ),
                ).build()
        FuzzHarnessBuildPlugin().execute(context)
    }

    /**
     * Generates both targets of one comparison: the single-protocol server from the other checkout, linked
     * against that checkout's runtime, and [multiServer].
     */
    private fun generateAbHarness(
        suite: String,
        protocol: Protocol,
        multiModel: Model,
        multiServer: File,
        dest: File,
        singleRuntime: File,
    ) {
        val service = serviceOf(suite)
        val singleServer = output.resolve(suite).resolve(protocol.dir).resolve("single-server")
        check(singleServer.resolve("Cargo.toml").exists()) {
            "no single-protocol server at $singleServer; run the `single` side first"
        }
        dest.deleteRecursively()
        generateFuzzTarget(multiModel, service, multiServer, dest, name = "multi")
        // Last, so that the lexicon comes from the protocol under test.
        generateFuzzTarget(singleModel(suite, protocol), service, singleServer, dest, "single", singleRuntime)
    }

    @Test
    fun `generate one side of the single versus multi protocol fuzz harnesses`() {
        for (suite in suites) {
            val service = serviceOf(suite)
            val suiteDir = output.resolve(suite)
            when (side) {
                "single" ->
                    for (protocol in selected) {
                        val model = singleModel(suite, protocol)
                        val protocolDir = suiteDir.resolve(protocol.dir)
                        val server = generateServer(model, service, protocolDir.resolve("single-server"))
                        val harness = protocolDir.resolve("single-harness")
                        harness.deleteRecursively()
                        generateFuzzTarget(model, service, server, harness)
                    }

                "multi" -> {
                    if (System.getenv("MP_FUZZ_ISOLATE_PROTOCOLS") == "true") {
                        for (protocol in selected) {
                            val model = singleModel(suite, protocol)
                            val protocolDir = suiteDir.resolve(protocol.dir)
                            val server = generateServer(model, service, protocolDir.resolve("schema-server"))
                            if (singleRuntime != null) {
                                generateAbHarness(suite, protocol, model, server, protocolDir.resolve("ab-harness"), singleRuntime)
                            }
                        }
                        continue
                    }
                    // restJson1 claims every REST request that carries neither a body nor a `Content-Type`, so
                    // restXml is compared against a server serving every protocol but restJson1.
                    val variants =
                        mapOf(
                            "multi" to protocols,
                            "multi-xml" to protocols.filterNot { it.dir == "rest-json1" },
                        )
                    for ((name, served) in variants) {
                        val model = multiModel(suite, served)
                        val server = generateServer(model, service, suiteDir.resolve("$name-server"))
                        val harness = suiteDir.resolve("$name-harness")
                        harness.deleteRecursively()
                        generateFuzzTarget(model, service, server, harness)
                        if (singleRuntime == null) continue
                        for (protocol in selected.filter { it in served }) {
                            val abHarness =
                                when (name) {
                                    "multi" -> "ab-harness"
                                    else -> if (protocol.dir == "rest-xml") "ab-harness-xml" else continue
                                }
                            generateAbHarness(
                                suite,
                                protocol,
                                model,
                                server,
                                suiteDir.resolve(protocol.dir).resolve(abHarness),
                                singleRuntime,
                            )
                        }
                    }
                }

                else -> error("MP_FUZZ_SIDE must be `single` or `multi`, got `$side`")
            }
        }
    }
}
