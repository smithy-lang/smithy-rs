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
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.ModelSerializer
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TraitFactory
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamMember
import software.amazon.smithy.rust.codegen.server.smithy.RustServerCodegenPlugin
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.hasConstraintTrait
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File
import java.nio.file.Path
import java.security.MessageDigest

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

    /**
     * Whether this side's servers are generated with `codegen.schemaSerde`: the `multi` side by default.
     * Set `MP_FUZZ_AFTER_SCHEMA_SERDE=false` to compare the baseline against this checkout's legacy path.
     */
    private val sideSchemaSerde =
        side == "multi" &&
            when (val value = System.getenv("MP_FUZZ_AFTER_SCHEMA_SERDE")) {
                null, "", "true" -> true
                "false" -> false
                else -> error("MP_FUZZ_AFTER_SCHEMA_SERDE must be `true` or `false`, got `$value`")
            }
    private val output = File(System.getenv("MP_FUZZ_OUTPUT") ?: error("MP_FUZZ_OUTPUT is required"))
    private val models = File(System.getenv("MP_FUZZ_MODELS") ?: error("MP_FUZZ_MODELS is required"))
    private val multiProtocolModels =
        File(System.getenv("MP_FUZZ_MULTI_PROTOCOL_MODELS") ?: error("MP_FUZZ_MULTI_PROTOCOL_MODELS is required"))

    /** Resolves a fixture: multi-protocol models live with the server integration suite, the rest in [models]. */
    private fun modelFile(name: String): File =
        if (name.startsWith("multi-protocol")) multiProtocolModels.resolve(name) else models.resolve(name)

    private val suites = (System.getenv("MP_FUZZ_SUITES") ?: "pokemon,multiprotocol").split(",")
    private val singleRuntime = System.getenv("MP_FUZZ_SINGLE_RUNTIME")?.let { File(it) }
    private val selected =
        System.getenv("MP_FUZZ_PROTOCOLS")?.split(",")?.let { names -> protocols.filter { it.dir in names } }
            ?: protocols

    private fun assemble(vararg sources: Pair<String, String>): Model =
        Model.assembler().discoverModels().apply {
            sources.forEach { (name, text) -> addUnparsedModel(name, text) }
        }.assemble().unwrap()

    private data class Projection(val canonical: Model, val model: Model, val manifest: ObjectNode)

    private val projections = mutableMapOf<Model, Projection>()

    private fun sha256(text: String): String =
        MessageDigest.getInstance("SHA-256").digest(text.toByteArray(Charsets.UTF_8))
            .joinToString("") { "%02x".format(it) }

    private fun json(model: Model): ObjectNode = ModelSerializer.builder().build().serialize(model)

    /** Explicitly record a common-denominator model projection; this is not full-model coverage. */
    private fun project(
        suite: String,
        canonical: Model,
        served: List<Protocol>,
        sources: List<Pair<String, String>>,
    ): Model {
        val serviceId = ShapeId.from(serviceOf(suite))
        val omittedOperation = if (suite == "pokemon") "StreamPokemonRadio" else "Upload"
        val streamingOperation = if (suite == "pokemon") "CapturePokemon" else "Publish"
        val omittedMember = if (suite == "pokemon") "region" else "topic"
        val namespace = serviceId.namespace
        val omittedId = ShapeId.from("$namespace#$omittedOperation")
        val streamingId = ShapeId.from("$namespace#$streamingOperation")
        val inputId = canonical.expectShape(streamingId, OperationShape::class.java).input.get()
        val memberId = inputId.withMember(omittedMember)
        check(canonical.expectShape(serviceId, ServiceShape::class.java).operations.contains(omittedId))
        check(canonical.expectShape(inputId, StructureShape::class.java).getMember(omittedMember).isPresent)
        val modelProfile =
            System.getenv("MP_FUZZ_MODEL_PROFILE")?.split(",")?.map { dir ->
                protocols.single { it.dir == dir }
            } ?: served
        check(modelProfile.containsAll(served)) { "MP_FUZZ_MODEL_PROFILE must include every served protocol" }
        val protocolSpecificOperations =
            if (suite == "multiprotocol" &&
                modelProfile.all { it.dir in listOf("aws-json-10", "aws-json-11", "rest-json1") }
            ) {
                setOf(ShapeId.from("$namespace#DocumentValues"))
            } else {
                emptySet()
            }
        protocolSpecificOperations.forEach { canonical.expectShape(it, OperationShape::class.java) }
        val reduceEventStreamMember = modelProfile.any { it.dir in listOf("aws-json-10", "aws-json-11") }
        val transformer = ModelTransformer.create()
        val reduced =
            transformer.mapShapes(canonical) { shape ->
                when (shape.id) {
                    serviceId -> {
                        val builder = (shape as ServiceShape).toBuilder().removeOperation(omittedId)
                        protocols.forEach { builder.removeTrait(it.shapeId) }
                        served.forEach {
                            val id = ShapeId.from(it.shapeId)
                            val trait =
                                canonical.expectShape(serviceId, ServiceShape::class.java).allTraits[id]
                                    ?: TraitFactory.createServiceFactory().createTrait(id, serviceId, Node.objectNode()).get()
                            builder.addTrait(trait)
                        }
                        protocolSpecificOperations.forEach { builder.addOperation(it) }
                        builder.build()
                    }
                    inputId -> if (reduceEventStreamMember) (shape as StructureShape).toBuilder().removeMember(omittedMember).build() else shape
                    streamingId -> {
                        val operation = shape as OperationShape
                        if (suite == "pokemon" && reduceEventStreamMember) {
                            val http = operation.expectTrait(HttpTrait::class.java)
                            operation.toBuilder().addTrait(http.toBuilder().uri(UriPattern.parse("/capture-pokemon-event")).build()).build()
                        } else {
                            operation
                        }
                    }
                    else -> shape
                }
            }
        // Match the generated server's validation error surface for every constrained operation,
        // including newly added structure/union payload inputs. Record each added error in the diff.
        val walker = DirectedWalker(reduced)
        val serviceOperations =
            TopDownIndex.of(reduced)
                .getContainedOperations(reduced.expectShape(serviceId, ServiceShape::class.java)).map { it.id }.toSet()
        val result =
            transformer.mapShapes(reduced) { shape ->
                if (shape is OperationShape && shape.id in serviceOperations && shape.input.isPresent &&
                    walker.walkShapes(reduced.expectShape(shape.input.get())).any {
                        it is SetShape || it is EnumShape || it.hasConstraintTrait() || it.hasEventStreamMember(reduced)
                    }
                ) {
                    shape.toBuilder().addError(ShapeId.from("smithy.framework#ValidationException")).build()
                } else {
                    shape
                }
            }
        val originalOperations =
            TopDownIndex.of(canonical)
                .getContainedOperations(canonical.expectShape(serviceId, ServiceShape::class.java)).map { it.id }.toSet()
        val projectedOperations =
            TopDownIndex.of(result)
                .getContainedOperations(result.expectShape(serviceId, ServiceShape::class.java)).map { it.id }.toSet()
        check(originalOperations - projectedOperations == setOf(omittedId))
        check(projectedOperations - originalOperations == protocolSpecificOperations)
        val originalMembers = canonical.expectShape(inputId, StructureShape::class.java).allMembers.keys
        val projectedMembers = result.expectShape(inputId, StructureShape::class.java).allMembers.keys
        check(originalMembers - projectedMembers == if (reduceEventStreamMember) setOf(omittedMember) else emptySet<String>())
        check(projectedMembers - originalMembers == emptySet<String>())
        val before = json(canonical)
        val after = json(result)
        val beforeShapes = before.expectObjectMember("shapes")
        val afterShapes = after.expectObjectMember("shapes")
        val shapeNames = (beforeShapes.members.keys + afterShapes.members.keys).map { it.value }.toSortedSet()
        val changes =
            shapeNames.mapNotNull { name ->
                val old = beforeShapes.getMember(name).orElse(Node.nullNode())
                val new = afterShapes.getMember(name).orElse(Node.nullNode())
                if (old == new) {
                    null
                } else {
                    Node.objectNodeBuilder().withMember("shape", name)
                        .withMember("before", old).withMember("after", new).build()
                }
            }
        val exclusions =
            listOf(
                Node.objectNodeBuilder().withMember("kind", "operation").withMember("shape", omittedId.toString())
                    .withMember("reason", "Generic fuzz handler does not implement plain streaming blob input/output")
                    .withMember("scope", "generic A/B fuzz target, both sides, every served protocol; REST streaming is separately tested")
                    .withMember("model_validity", "This exclusion does not imply that the Smithy model or REST streaming is unsupported")
                    .build(),
            ) +
                if (reduceEventStreamMember) {
                    listOf(
                        Node.objectNodeBuilder().withMember("kind", "input_member").withMember("shape", memberId.toString())
                            .withMember("reason", "Legacy AWS JSON event-stream input codegen requires the stream to be the sole input member")
                            .withMember("scope", "Both sides of profiles serving AWS JSON; retained by CBOR-only and REST-only profiles")
                            .withMember("model_validity", "REST supports additional bound input members; this is a legacy AWS JSON codegen restriction, not a blanket RPC invalidity claim")
                            .build(),
                    )
                } else {
                    emptyList()
                }
        val sourceHashes =
            sources.map { (name, text) ->
                Node.objectNodeBuilder().withMember("path", modelFile(name).absolutePath)
                    .withMember("sha256", sha256(text)).build()
            }
        val comparisonModel =
            transformer.mapShapes(result) { shape ->
                if (shape.id == serviceId) {
                    val builder = (shape as ServiceShape).toBuilder()
                    protocols.forEach { builder.removeTrait(it.shapeId) }
                    builder.build()
                } else {
                    shape
                }
            }
        val comparisonNode =
            ModelSerializer.builder().shapeFilter {
                it.id.namespace in setOf(namespace, "smithy.framework")
            }.build().serialize(comparisonModel)
        val manifest =
            Node.objectNodeBuilder().withMember("version", 1).withMember("suite", suite)
                .withMember("side", side).withMember("coverage_scope", "partial common-denominator model; not full canonical model coverage")
                .withMember("schemaSerde", sideSchemaSerde)
                .withMember("generator_server_class_location", RustServerCodegenPlugin::class.java.protectionDomain.codeSource.location.toString())
                .withMember("generator_runtime_root", File(TestRuntimeConfig.runtimeCrateLocation.path!!).canonicalPath)
                .withMember("baseline_provenance", "Side and model hashes do not establish a historical baseline; record the selected generator checkout separately")
                .withMember("model_stage", "Assembled canonical server model to explicit fuzz projection, before server/fuzz normalization")
                .withMember("served_protocols", ArrayNode.fromNodes(served.map { Node.from(it.shapeId) }))
                .withMember("model_profile_protocols", ArrayNode.fromNodes(modelProfile.map { Node.from(it.shapeId) }))
                .withMember("source_files", ArrayNode.fromNodes(sourceHashes))
                .withMember("canonical_model_sha256", sha256(Node.prettyPrintJson(before)))
                .withMember("fuzz_model_sha256", sha256(Node.prettyPrintJson(after)))
                .withMember("comparison_model_sha256", sha256(Node.prettyPrintJson(comparisonNode)))
                .withMember("canonical_operation_count", originalOperations.size)
                .withMember("fuzz_operation_count", projectedOperations.size)
                .withMember("excluded_operation_count", 1).withMember("excluded_input_member_count", if (reduceEventStreamMember) 1 else 0)
                .withMember("protocol_specific_operation_count", protocolSpecificOperations.size)
                .withMember(
                    "protocol_specific_additions",
                    ArrayNode.fromNodes(
                        protocolSpecificOperations.map {
                            Node.objectNodeBuilder().withMember("shape", it.toString())
                                .withMember("reason", "JSON-isolated document-value case declared by the protocol-specific canonical service")
                                .withMember("scope", "JSON-only profiles; Document is not supported by canonical XML/CBOR mixed service").build()
                        },
                    ),
                )
                .withMember("unasserted_canonical_operation_count", 1)
                .withMember("excluded_canonical_members", ArrayNode.fromNodes(if (reduceEventStreamMember) listOf(Node.from(memberId.toString())) else emptyList()))
                .withMember("exclusions", ArrayNode.fromNodes(exclusions))
                .withMember("model_diff", ArrayNode.fromNodes(changes)).build()
        check(!reduceEventStreamMember || changes.any { it.expectStringMember("shape").value == inputId.toString() })
        val projection = Projection(canonical, result, manifest)
        projections[result] = projection
        val profile =
            served.joinToString("+") { it.dir } +
                if (modelProfile != served) "__model-" + modelProfile.joinToString("+") { it.dir } else ""
        writeProjection(projection, output.resolve(suite).resolve("model-projections/$side/$profile"))
        return result
    }

    private fun writeProjection(
        projection: Projection,
        dest: File,
    ) {
        dest.mkdirs()
        dest.resolve("canonical-model.json").writeText(Node.prettyPrintJson(json(projection.canonical)) + "\n")
        dest.resolve("fuzz-model.json").writeText(Node.prettyPrintJson(json(projection.model)) + "\n")
        dest.resolve("model-projection.json").writeText(Node.prettyPrintJson(projection.manifest) + "\n")
    }

    private fun pokemon(with: List<Protocol>): Model {
        val sources = listOf("pokemon.smithy", "pokemon-common.smithy").map { it to modelFile(it).readText() }
        return project("pokemon", assemble(*sources.toTypedArray()), with, sources)
    }

    private fun multiProtocol(
        file: String,
        served: List<Protocol>,
    ): Model {
        // Canonical server service includes Upload even when RPC client declarations omit it.
        // Hash the exact text used for assembly, even if another task edits fixtures concurrently.
        val sources =
            listOf("multi-protocol-server.smithy", "multi-protocol-common.smithy", file).distinct()
                .map { it to modelFile(it).readText() }
        val canonicalSources =
            sources.filter {
                it.first in listOf("multi-protocol-server.smithy", "multi-protocol-common.smithy")
            }
        return project("multiprotocol", assemble(*canonicalSources.toTypedArray()), served, sources)
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
    ) =
        if (suite == "pokemon") pokemon(listOf(protocol)) else multiProtocol(protocol.multiProtocolFile, listOf(protocol))

    private fun multiModel(
        suite: String,
        served: List<Protocol>,
    ): Model =
        if (suite == "pokemon") {
            pokemon(served)
        } else {
            multiProtocol("multi-protocol-server.smithy", served)
        }

    /** Generates an HTTP 1.x server for [model] and copies the crate to [dest]. */
    private fun generateServer(
        model: Model,
        service: String,
        dest: File,
    ): File {
        val codegen = Node.objectNodeBuilder().withMember("http-1x", true)
        codegen.withMember("schemaSerde", sideSchemaSerde)
        val servers =
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    service = service,
                    additionalSettings = Node.objectNode().withMember("codegen", codegen.build()),
                    command = { dir -> println("generated $dir") },
                ),
                testCoverage = HttpTestType.Default,
            ) { context, _ ->
                val normalizedModel = EventStreamNormalizer.transform(OperationNormalizer.transform(model))
                TopDownIndex.of(model).getContainedOperations(model.expectShape(ShapeId.from(service), ServiceShape::class.java))
                    .map { it.id }.forEach { id ->
                        check(
                            normalizedModel.expectShape(id, OperationShape::class.java).errors.toSet() ==
                                context.model.expectShape(id, OperationShape::class.java).errors.toSet(),
                        ) {
                            "Fuzz projection and generated server error surfaces differ for $id: " +
                                "${normalizedModel.expectShape(id, OperationShape::class.java).errors} vs " +
                                "${context.model.expectShape(id, OperationShape::class.java).errors}"
                        }
                    }
            }
        dest.deleteRecursively()
        servers.single().path.toFile().copyRecursively(dest)
        dest.resolve("target").deleteRecursively()
        writeProjection(projections.getValue(model), dest.resolve("model-projection"))
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
        // A paired target is generated by the multi-side test even when it links a single-side SDK.
        // Preserve that SDK's archived settings instead of relabeling it from this process's side.
        val serverProjection = server.resolve("model-projection")
        check(serverProjection.resolve("model-projection.json").exists()) {
            "Generated server has no archived model/settings provenance: $server"
        }
        val targetProjection = dest.resolve("model-projections/$name")
        targetProjection.deleteRecursively()
        serverProjection.copyRecursively(targetProjection)
        val provenance =
            Node.parse(targetProjection.resolve("model-projection.json").readText()).expectObjectNode()
                .withMember("fuzz_generator_side", side)
                .withMember("target_name", name)
                .withMember("linked_server", server.canonicalPath)
                .withMember("linked_runtime", runtime.canonicalPath)
        targetProjection.resolve("model-projection.json").writeText(Node.prettyPrintJson(provenance) + "\n")
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
        val single = singleModel(suite, protocol)
        val beforeManifest = singleServer.resolve("model-projection/model-projection.json")
        check(beforeManifest.exists()) {
            "Regenerate the baseline server with an explicit model projection manifest: $singleServer"
        }
        val baseline = Node.parse(beforeManifest.readText()).expectObjectNode()
        val expected = projections.getValue(single).manifest.expectStringMember("comparison_model_sha256").value
        val candidate = projections.getValue(multiModel).manifest.expectStringMember("comparison_model_sha256").value
        check(baseline.expectStringMember("comparison_model_sha256").value == expected) {
            "Baseline server model changed; regenerate matching inputs for $suite/${protocol.dir}"
        }
        check(expected == candidate) {
            "A/B profile models differ for $suite/${protocol.dir}; generate a matching baseline profile or use isolated protocols. " +
                "Protocol-only DocumentValues and retained topic/region cannot be silently removed to make a comparison pass."
        }
        dest.deleteRecursively()
        generateFuzzTarget(multiModel, service, multiServer, dest, name = "multi")
        // Last, so that the lexicon comes from the protocol under test.
        generateFuzzTarget(single, service, singleServer, dest, "single", singleRuntime)
    }

    /** Exercises projection invariants without claiming execution of omitted operations. */
    @Test
    fun verifyModelProjectionManifest() {
        for (suite in suites) {
            selected.forEach { singleModel(suite, it) }
            multiModel(suite, protocols)
            multiModel(suite, protocols.filter { it.dir in listOf("rest-json1", "rest-xml") })
        }
        check(projections.isNotEmpty())
        check(projections.values.all { it.manifest.expectNumberMember("excluded_operation_count").value.toInt() == 1 })
    }

    @Test
    @EnabledIfEnvironmentVariable(named = "MP_FUZZ_MODEL_PROFILE", matches = ".+")
    fun verifyMatchingModelProfiles() {
        val profile = System.getenv("MP_FUZZ_MODEL_PROFILE").split(",").map { dir -> protocols.single { it.dir == dir } }
        for (suite in suites) {
            val candidate = projections.getValue(multiModel(suite, profile)).manifest
            for (protocol in selected.filter { it in profile }) {
                val baseline = projections.getValue(singleModel(suite, protocol)).manifest
                check(
                    baseline.expectStringMember("comparison_model_sha256").value ==
                        candidate.expectStringMember("comparison_model_sha256").value,
                ) {
                    "Explicit baseline model profile differs from candidate for $suite/${protocol.dir}"
                }
            }
        }
    }

    private fun canonicalFocusedModel(
        protocol: Protocol,
        operation: String,
    ): Model {
        val canonical =
            assemble(
                protocol.multiProtocolFile to modelFile(protocol.multiProtocolFile).readText(),
                "multi-protocol-common.smithy" to modelFile("multi-protocol-common.smithy").readText(),
            )
        val serviceId = ShapeId.from(serviceOf("multiprotocol"))
        return ModelTransformer.create().mapShapes(canonical) { shape ->
            if (shape.id == serviceId) {
                (shape as ServiceShape).toBuilder()
                    .operations(listOf(ShapeId.from("${serviceId.namespace}#$operation"))).build()
            } else {
                shape
            }
        }
    }

    /** Reproduce legacy RPC generation restrictions with the original event-stream input member. */
    @Test
    @EnabledIfEnvironmentVariable(named = "MP_FUZZ_VERIFY_EXCLUSIONS", matches = "true")
    fun verifyOriginalRpcEventStreamGeneration() {
        val records =
            protocols.map { protocol ->
                val model = canonicalFocusedModel(protocol, "Publish")
                val dest = output.resolve("excluded-case-verification/${protocol.dir}-original-publish")
                dest.mkdirs()
                dest.resolve("model.json").writeText(Node.prettyPrintJson(json(model)))
                val failure =
                    runCatching {
                        serverIntegrationTest(
                            model,
                            IntegrationTestParams(
                                service = serviceOf("multiprotocol"),
                                additionalSettings = Node.parse("""{"codegen":{"schemaSerde":false}}""").expectObjectNode(),
                                command = { dir -> println("generated original Publish at $dir") },
                            ),
                            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
                        )
                    }.exceptionOrNull()
                val details = generateSequence(failure) { it.cause }.joinToString("\n") { "${it.javaClass.name}: ${it.message}" }
                if (protocol.dir in listOf("rpcv2-cbor", "rest-json1", "rest-xml")) {
                    check(failure == null) { "Canonical Publish.topic must generate for current-tree legacy ${protocol.dir}; got $details" }
                } else {
                    check(failure != null && details.contains("We only support one payload member")) {
                        "Expected the observed legacy AWS JSON streaming-input restriction for ${protocol.dir}; got $details"
                    }
                }
                dest.resolve("generation-failure.txt").writeText(details + "\n")
                Node.objectNodeBuilder().withMember("protocol", protocol.shapeId).withMember("schemaSerde", false)
                    .withMember("operation", "com.example.multiprotocol#Publish")
                    .withMember("topic_preserved", true).withMember("status", if (failure == null) "generation_supported_runtime_unverified" else "blocked_by_legacy_codegen")
                    .withMember("executed_handler_assertion_count", 0).withMember("failure", details).build()
            }
        output.resolve("excluded-case-verification").resolve("rpc-generation-results.json")
            .writeText(Node.prettyPrintJson(ArrayNode.fromNodes(records)) + "\n")
    }

    /** Plain blob streaming is excluded by the generic fuzz handler, but supported REST servers must execute it. */
    @Test
    @EnabledIfEnvironmentVariable(named = "MP_FUZZ_VERIFY_EXCLUSIONS", matches = "true")
    fun verifyCanonicalRestUploadBytes() {
        for (protocol in protocols.filter { it.dir in listOf("rest-json1", "rest-xml") }) {
            for (schemaSerde in listOf(false, true)) {
                val model = canonicalFocusedModel(protocol, "Upload")
                val dest = output.resolve("excluded-case-verification/${protocol.dir}-upload-$schemaSerde")
                dest.mkdirs()
                dest.resolve("model.json").writeText(Node.prettyPrintJson(json(model)))
                val records = dest.resolve("wire.tsv")
                val generated =
                    serverIntegrationTest(
                        model,
                        IntegrationTestParams(
                            service = serviceOf("multiprotocol"),
                            additionalSettings = Node.parse("""{"codegen":{"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                            cargoCommand = "cargo test --quiet --lib --tests --offline",
                        ),
                        testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
                    ) { context, crate ->
                        crate.testModule {
                            tokioTest("canonical_rest_upload_preserves_bytes") {
                                rustTemplate(
                                    """
                                    let config = crate::service::MultiProtocolServiceConfig::builder().build();
                                    let service = crate::service::MultiProtocolService::builder(config)
                                        .upload(|input: crate::input::UploadInput| async move {
                                            let actual = input.data.collect().await.unwrap().into_bytes();
                                            assert!(actual.is_empty() || actual.as_ref() == &[0, 255, b'<', b'&', 10, b'"']);
                                            #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput { size: actual.len() as i64 })
                                        }).build_unchecked();
                                    let mut records = #{String}::from("case\tstatus\theaders\tbodyHex\n");
                                    for bytes in [vec![], vec![0u8, 255, b'<', b'&', 10, b'"']] {
                                        let request = #{Http}::Request::builder().method("PUT").uri("/upload")
                                            .header("content-type", "application/octet-stream")
                                            .body(#{Server}::body::to_boxed_sync(bytes.clone())).unwrap();
                                        let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                        assert_eq!(response.status().as_u16(), 200);
                                        let headers = format!("{:?}", response.headers());
                                        let body = #{BodyUtil}::BodyExt::collect(#{BodyUtil}::Limited::new(response.into_body(), 1024)).await.unwrap().to_bytes();
                                        let expected = if "${protocol.dir}" == "rest-json1" {
                                            format!("{{\"size\":{}}}", bytes.len())
                                        } else {
                                            format!("<UploadOutput><size>{}</size></UploadOutput>", bytes.len())
                                        };
                                        assert_eq!(body.as_ref(), expected.as_bytes());
                                        records.push_str(&format!("{}\t200\t{}\t{}\n", bytes.len(), headers,
                                            body.iter().map(|byte| format!("{byte:02x}")).collect::<#{String}>()));
                                    }
                                    ::std::fs::write("${records.absolutePath}", records).unwrap();
                                    """,
                                    *RuntimeType.preludeScope,
                                    "Http" to RuntimeType.http(context.runtimeConfig),
                                    "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                                    "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                    "Tower" to ServerCargoDependency.tower(context).toType(),
                                )
                            }
                        }
                    }
                generated.single().path.toFile().copyRecursively(dest.resolve("generated-server"), overwrite = true)
                dest.resolve("generated-server/target").deleteRecursively()
            }
            val legacy = output.resolve("excluded-case-verification/${protocol.dir}-upload-false/wire.tsv").readText()
            val schema = output.resolve("excluded-case-verification/${protocol.dir}-upload-true/wire.tsv").readText()
            check(legacy == schema) { "Canonical Upload complete status/headers/body differs for ${protocol.dir}" }
        }
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
                    // `MP_FUZZ_VARIANTS` overrides the served protocol sets, e.g.
                    // `bunch-json=aws-json-10+rest-json1+rpcv2-cbor;bunch-xml=aws-json-11+rest-xml+rpcv2-cbor`;
                    // each variant then gets an `ab-harness-<name>` per served protocol.
                    val customVariants = System.getenv("MP_FUZZ_VARIANTS")
                    val variants =
                        customVariants?.split(";")?.associate { entry ->
                            val (name, dirs) = entry.split("=", limit = 2)
                            name to dirs.split("+").map { dir -> protocols.single { it.dir == dir } }
                        }
                            ?: mapOf(
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
                                when {
                                    customVariants != null -> "ab-harness-$name"
                                    name == "multi" -> "ab-harness"
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
