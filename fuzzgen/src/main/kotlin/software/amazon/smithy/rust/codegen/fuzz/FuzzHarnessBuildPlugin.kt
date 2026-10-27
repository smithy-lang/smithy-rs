/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.fuzz

import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.build.SmithyBuildPlugin
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.NumberNode
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpPrefixHeadersTrait
import software.amazon.smithy.model.traits.HttpQueryTrait
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.JsonNameTrait
import software.amazon.smithy.model.traits.XmlNameTrait
import software.amazon.smithy.protocoltests.traits.HttpRequestTestsTrait
import software.amazon.smithy.rust.codegen.core.generated.BuildEnvironment
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.server.smithy.transformers.AttachValidationExceptionToConstrainedOperationInputsInAllowList
import java.io.File
import java.nio.file.Path
import java.util.Base64
import kotlin.streams.toList

/**
 * Metadata for a TargetCrate: A code generated smithy-rs server for a given model
 */
data class TargetCrate(
    /** The name of the Fuzz target */
    val name: String,
    /** Where the server implementation of this target is */
    val relativePath: String,
) {
    companion object {
        fun fromNode(node: ObjectNode): TargetCrate {
            val name = node.expectStringMember("name").value
            val relativePath = node.expectStringMember("relativePath").value
            return TargetCrate(name, relativePath)
        }
    }

    /** The name of the actual `package` from Cargo's perspective.
     *
     *  We need this to make a dependency on it
     * */
    fun targetPackage(): String {
        val path = Path.of(relativePath)
        val cargoToml = path.resolve("Cargo.toml").toFile()
        val packageSection = cargoToml.readLines().dropWhile { it.trim() != "[package]" }
        return packageSection.firstOrNull { it.startsWith("name =") }?.let { it.split("=")[1].trim() }?.trim('"')
            ?: throw Exception("no package name")
    }
}

data class FuzzSettings(
    val targetServers: List<TargetCrate>,
    val service: ShapeId,
    val runtimeConfig: RuntimeConfig,
) {
    companion object {
        fun fromNode(node: ObjectNode): FuzzSettings {
            val targetCrates =
                node.expectArrayMember("targetCrates")
                    .map { TargetCrate.fromNode(it.expectObjectNode()) }
            val service = ShapeId.fromNode(node.expectStringMember("service"))
            val runtimeConfig = RuntimeConfig.fromNode(node.getObjectMember("runtimeConfig"))
            return FuzzSettings(targetCrates, service, runtimeConfig)
        }
    }
}

/**
 * Build plugin for generating a fuzz harness and lexicon from a smithy model and a set of smithy-rs versions
 *
 * This is used by `aws-smithy-fuzz` which contains most of the usage docs
 */
class FuzzHarnessBuildPlugin : SmithyBuildPlugin {
    // `aws-smithy-fuzz` is not part of the `rust-runtime` workspace,
    // and its dependencies may not be included in the SDK lockfile.
    // This plugin needs to use the lockfile from `aws-smithy-fuzz`.
    private val cargoLock: File by lazy {
        File(BuildEnvironment.PROJECT_DIR).resolve("rust-runtime/aws-smithy-fuzz/Cargo.lock")
    }

    override fun getName(): String = "fuzz-harness"

    override fun execute(context: PluginContext) {
        val fuzzSettings = FuzzSettings.fromNode(context.settings)

        val model =
            context.model.let(OperationNormalizer::transform)
                .let(AttachValidationExceptionToConstrainedOperationInputsInAllowList::transform)
        val targets =
            fuzzSettings.targetServers.map { target ->
                val targetContext = createFuzzTarget(target, context.fileManifest, fuzzSettings, model)
                println("Creating a fuzz targret for $targetContext")
                FuzzTargetGenerator(targetContext).generateFuzzTarget()
                targetContext
            }

        println("creating the driver...")
        createDriver(model, context.fileManifest, fuzzSettings)

        targets.forEach {
            val manifest = it.finalize()
            context.fileManifest.addAllFiles(manifest)
            // A fuzz target crate exists in a nested structure like:
            // smithy-test-workspace/smithy-test4510328876569901367/a
            //
            // Each fuzz target is its own workspace with `[workspace]\n_ignored _ignored`.
            // As a result, placing the lockfile from `aws-smithy-fuzz` in `TestWorkspace`
            // has no effect; it must be copied directly into the fuzz target.
            cargoLock.copyTo(manifest.baseDir.resolve("Cargo.lock").toFile(), true)
        }
    }
}

/**
 * Generate a corpus of words used within the model to seed the dictionary
 */
fun corpus(
    model: Model,
    fuzzSettings: FuzzSettings,
): ArrayNode {
    val operations = TopDownIndex.of(model).getContainedOperations(fuzzSettings.service)
    val protocolTests = operations.flatMap { it.getTrait<HttpRequestTestsTrait>()?.testCases ?: listOf() }
    val out = ArrayNode.builder()
    protocolTests.forEach { testCase ->
        val body: List<NumberNode> =
            when (testCase.bodyMediaType.orNull()) {
                "application/cbor" -> {
                    println("base64 decoding first (v2)")
                    Base64.getDecoder().decode(testCase.body.orNull())?.map { NumberNode.from(it.toUByte().toInt()) }
                }

                else -> testCase.body.orNull()?.chars()?.toList()?.map { c -> NumberNode.from(c) }
            } ?: listOf()
        out.withValue(
            ObjectNode.objectNode()
                .withMember("uri", testCase.uri)
                .withMember("method", testCase.method)
                .withMember(
                    "headers",
                    ObjectNode.objectNode(
                        testCase.headers.map { (k, v) ->
                            (StringNode.from(k) to ArrayNode.fromStrings(v))
                        }.toMap(),
                    ),
                )
                .withMember("trailers", ObjectNode.objectNode())
                .withMember(
                    "body",
                    ArrayNode.fromNodes(body),
                ),
        )
    }
    return out.build()
}

fun createDriver(
    model: Model,
    baseManifest: FileManifest,
    fuzzSettings: FuzzSettings,
) {
    val fuzzLexicon =
        ObjectNode.objectNode()
            .withMember("corpus", corpus(model, fuzzSettings))
            .withMember("dictionary", dictionary(model, fuzzSettings))
    baseManifest.writeFile("lexicon.json", Node.prettyPrintJson(fuzzLexicon))
}

fun dictionary(
    model: Model,
    fuzzSettings: FuzzSettings,
): ArrayNode {
    val operations = TopDownIndex.of(model).getContainedOperations(fuzzSettings.service)
    val walker = Walker(model)
    val dictionary = mutableSetOf<String>()
    operations.forEach {
        walker.iterateShapes(it).forEach { shape ->
            dictionary.addAll(getTraitBasedNames(shape))
            dictionary.add(shape.id.name)
            when (shape) {
                is MemberShape -> dictionary.add(shape.memberName)
                is OperationShape -> dictionary.add(shape.id.toString())
                else -> {}
            }
        }
    }
    return ArrayNode.fromStrings(dictionary.toList().sorted())
}

fun getTraitBasedNames(shape: Shape): List<String> {
    return listOfNotNull(
        shape.getTrait<JsonNameTrait>()?.value,
        shape.getTrait<XmlNameTrait>()?.value,
        shape.getTrait<HttpQueryTrait>()?.value,
        shape.getTrait<HttpTrait>()?.method,
        *(
            shape.getTrait<HttpTrait>()?.uri?.queryLiterals?.flatMap { (k, v) -> listOf(k, v) }
                ?: listOf()
        ).toTypedArray(),
        shape.getTrait<HttpPrefixHeadersTrait>()?.value,
    )
}

class NoOpDocProvider : ModuleDocProvider {
    override fun docsWriter(module: RustModule.LeafModule): Writable? {
        return null
    }
}
