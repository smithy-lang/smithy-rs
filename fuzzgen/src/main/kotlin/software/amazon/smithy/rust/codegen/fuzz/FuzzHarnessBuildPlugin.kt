/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.fuzz

import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.build.SmithyBuildPlugin
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.NumberNode
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.HttpPrefixHeadersTrait
import software.amazon.smithy.model.traits.HttpQueryTrait
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.JsonNameTrait
import software.amazon.smithy.model.traits.XmlNameTrait
import software.amazon.smithy.protocoltests.traits.HttpRequestTestsTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.BasicRustCrate
import software.amazon.smithy.rust.codegen.core.smithy.CoreCodegenConfig
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.ModuleProviderContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.server.smithy.ServerModuleProvider
import software.amazon.smithy.rust.codegen.server.smithy.transformers.AttachValidationExceptionToConstrainedOperationInputsInAllowList
import java.nio.file.Path
import kotlin.streams.toList

data class FuzzTarget(val name: String, val relativePath: String) {
    companion object {
        fun fromNode(node: ObjectNode): FuzzTarget {
            val name = node.expectStringMember("name").value
            val relativePath = node.expectStringMember("relativePath").value
            return FuzzTarget(name, relativePath)
        }
    }

    fun targetPackage(): String {
        val path = Path.of(relativePath)
        val cargoToml = path.resolve("Cargo.toml").toFile()
        val packageSection = cargoToml.readLines().dropWhile { it.trim() != "[package]" }
        return packageSection.firstOrNull { it.startsWith("name =") }?.let { it.split("=")[1].trim() }?.trim('"')
            ?: throw Exception("no package name")
    }
}

data class FuzzSettings(
    val targetCratePath: List<FuzzTarget>,
    val service: ShapeId,
    val runtimeConfig: RuntimeConfig,
) {
    companion object {
        fun fromNode(node: ObjectNode): FuzzSettings {
            val targetCrates = node.expectArrayMember("targetCrates").map { FuzzTarget.fromNode(it.expectObjectNode()) }
            val service = ShapeId.fromNode(node.expectStringMember("service"))
            val runtimeConfig = RuntimeConfig.fromNode(node.getObjectMember("runtimeConfig"))
            return FuzzSettings(targetCrates, service, runtimeConfig)
        }
    }
}

class FuzzHarnessBuildPlugin : SmithyBuildPlugin {
    override fun getName(): String = "fuzz-harness"

    override fun execute(context: PluginContext) {
        val fuzzSettings = FuzzSettings.fromNode(context.settings)

        val subdir = FileManifest.create(context.fileManifest.resolvePath(Path.of("driver")))
        val model =
            context.model.let(OperationNormalizer::transform)
                .let(AttachValidationExceptionToConstrainedOperationInputsInAllowList::transform)
        val targets =
            fuzzSettings.targetCratePath.map { target ->
                val target = createFuzzTarget(target, context.fileManifest, fuzzSettings, model)
                FuzzTargetGenerator(target).generateFuzzTarget()
                target
            }

        createDriver(model, context.fileManifest, fuzzSettings)

        targets.forEach {
            context.fileManifest.addAllFiles(it.finalize())
        }
    }
}

fun driverSettings(
    service: ShapeId,
    runtimeConfig: RuntimeConfig,
) = CoreRustSettings(
    service,
    moduleVersion = "0.1.0",
    moduleName = "fuzz-driver",
    moduleAuthors = listOf(),
    codegenConfig = CoreCodegenConfig(),
    license = null,
    runtimeConfig = runtimeConfig,
    moduleDescription = null,
    moduleRepository = null,
)

fun corpus(
    model: Model,
    fuzzSettings: FuzzSettings,
): ArrayNode {
    val operations = TopDownIndex.of(model).getContainedOperations(fuzzSettings.service)
    val protocolTests = operations.flatMap { it.getTrait<HttpRequestTestsTrait>()?.testCases ?: listOf() }
    val out = ArrayNode.builder()
    protocolTests.forEach { testCase ->
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
                    ArrayNode.fromNodes(
                        testCase.body.orNull()?.chars()?.toList()?.map { c -> NumberNode.from(c) }
                            ?: listOf(),
                    ),
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

fun createFuzzTarget(
    target: FuzzTarget,
    baseManifest: FileManifest,
    fuzzSettings: FuzzSettings,
    model: Model,
): FuzzTargetContext {
    val newManifest = FileManifest.create(baseManifest.resolvePath(Path.of(target.name)))
    val codegenConfig = CoreCodegenConfig()
    val symbolProvider =
        SymbolVisitor(
            rustSettings(fuzzSettings, target),
            model,
            model.expectShape(fuzzSettings.service, ServiceShape::class.java),
            RustSymbolProviderConfig(
                fuzzSettings.runtimeConfig,
                renameExceptions = false,
                NullableIndex.CheckMode.SERVER,
                ServerModuleProvider,
            ),
        ).let { PublicCrateSymbolProvider("rust_server_codegen", it) }
    val crate =
        BasicRustCrate(
            newManifest,
            symbolProvider,
            codegenConfig,
            DocProvider(),
        )
    return FuzzTargetContext(
        target = target,
        fuzzSettings = fuzzSettings,
        rustCrate = crate,
        model = model,
        manifest = newManifest,
        symbolProvider = symbolProvider,
    )
}

class DocProvider : ModuleDocProvider {
    override fun docsWriter(module: RustModule.LeafModule): Writable? {
        return null
    }
}

class NoOpVisitor : RustSymbolProvider {
    override val model: Model
        get() = TODO("Not yet implemented")
    override val moduleProviderContext: ModuleProviderContext
        get() = TODO("Not yet implemented")
    override val config: RustSymbolProviderConfig
        get() = TODO("Not yet implemented")

    override fun symbolForOperationError(operation: OperationShape): Symbol {
        TODO("Not yet implemented")
    }

    override fun symbolForEventStreamError(eventStream: UnionShape): Symbol {
        TODO("Not yet implemented")
    }

    override fun symbolForBuilder(shape: Shape): Symbol {
        TODO("Not yet implemented")
    }

    override fun toSymbol(shape: Shape?): Symbol {
        TODO("Not yet implemented")
    }
}

class PublicCrateSymbolProvider(private val crateName: String, private val base: RustSymbolProvider) :
    WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        val base = base.toSymbol(shape)
        return base.mapRustType { ty ->
            when (ty) {
                is RustType.Opaque -> RustType.Opaque(ty.name, ty.namespace?.replace("crate", crateName))
                else -> ty
            }
        }
    }
}
