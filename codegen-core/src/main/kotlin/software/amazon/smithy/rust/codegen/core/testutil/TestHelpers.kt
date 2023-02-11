/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWordSymbolProvider
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.BaseSymbolMetadataProvider
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.CoreCodegenConfig
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.ModuleProvider
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeCrateLocation
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import java.io.File

val TestRuntimeConfig =
    RuntimeConfig(runtimeCrateLocation = RuntimeCrateLocation.Path(File("../rust-runtime/").absolutePath))

/**
 * IMPORTANT: You shouldn't need to refer to these directly in code or tests. They are private for a reason.
 *
 * In general, the RustSymbolProvider's `config()` has a `moduleFor` function that should be used
 * to find the destination module for a given shape.
 */
private object CodegenCoreTestModules {
    // Use module paths that don't align with either server or client to make sure
    // the codegen is resilient to differences in module path.
    val ModelsTestModule = RustModule.public("test_model", documentation = "Test models module")
    val ErrorsTestModule = RustModule.public("test_error", documentation = "Test error module")
    val InputsTestModule = RustModule.public("test_input", documentation = "Test input module")
    val OutputsTestModule = RustModule.public("test_output", documentation = "Test output module")
    val OperationsTestModule = RustModule.public("test_operation", documentation = "Test operation module")

    object TestModuleProvider : ModuleProvider {
        override fun moduleForShape(shape: Shape): RustModule.LeafModule = when (shape) {
            is OperationShape -> OperationsTestModule
            is StructureShape -> when {
                shape.hasTrait<ErrorTrait>() -> ErrorsTestModule
                shape.hasTrait<SyntheticInputTrait>() -> InputsTestModule
                shape.hasTrait<SyntheticOutputTrait>() -> OutputsTestModule
                else -> ModelsTestModule
            }
            else -> ModelsTestModule
        }

        override fun moduleForOperationError(operation: OperationShape): RustModule.LeafModule = ErrorsTestModule
        override fun moduleForEventStreamError(eventStream: UnionShape): RustModule.LeafModule = ErrorsTestModule
    }
}

val TestSymbolVisitorConfig = SymbolVisitorConfig(
    runtimeConfig = TestRuntimeConfig,
    renameExceptions = true,
    nullabilityCheckMode = NullableIndex.CheckMode.CLIENT_ZERO_VALUE_V1,
    moduleProvider = CodegenCoreTestModules.TestModuleProvider,
)

fun testRustSettings(
    service: ShapeId = ShapeId.from("notrelevant#notrelevant"),
    moduleName: String = "test-module",
    moduleVersion: String = "0.0.1",
    moduleAuthors: List<String> = listOf("notrelevant"),
    moduleDescription: String = "not relevant",
    moduleRepository: String? = null,
    runtimeConfig: RuntimeConfig = TestRuntimeConfig,
    codegenConfig: CoreCodegenConfig = CoreCodegenConfig(),
    license: String? = null,
    examplesUri: String? = null,
) = CoreRustSettings(
    service,
    moduleName,
    moduleVersion,
    moduleAuthors,
    moduleDescription,
    moduleRepository,
    runtimeConfig,
    codegenConfig,
    license,
    examplesUri,
)

private const val SmithyVersion = "1.0"
fun String.asSmithyModel(sourceLocation: String? = null, smithyVersion: String = SmithyVersion): Model {
    val processed = letIf(!this.trimStart().startsWith("\$version")) { "\$version: ${smithyVersion.dq()}\n$it" }
    return Model.assembler().discoverModels().addUnparsedModel(sourceLocation ?: "test.smithy", processed).assemble()
        .unwrap()
}

// Intentionally only visible to codegen-core since the other modules have their own symbol providers
internal fun testSymbolProvider(model: Model): RustSymbolProvider = SymbolVisitor(
    model,
    ServiceShape.builder().version("test").id("test#Service").build(),
    TestSymbolVisitorConfig,
).let { BaseSymbolMetadataProvider(it, model, additionalAttributes = listOf(Attribute.NonExhaustive)) }
    .let { RustReservedWordSymbolProvider(it, model) }

// Intentionally only visible to codegen-core since the other modules have their own contexts
internal fun testCodegenContext(
    model: Model,
    serviceShape: ServiceShape? = null,
    settings: CoreRustSettings = testRustSettings(),
    codegenTarget: CodegenTarget = CodegenTarget.CLIENT,
): CodegenContext = CodegenContext(
    model,
    testSymbolProvider(model),
    serviceShape
        ?: model.serviceShapes.firstOrNull()
        ?: ServiceShape.builder().version("test").id("test#Service").build(),
    ShapeId.from("test#Protocol"),
    settings,
    codegenTarget,
)

/**
 * In tests, we frequently need to generate a struct, a builder, and an impl block to access said builder.
 */
fun StructureShape.renderWithModelBuilder(
    model: Model,
    symbolProvider: RustSymbolProvider,
    writer: RustWriter,
) {
    StructureGenerator(model, symbolProvider, writer, this, emptyList()).render()
    BuilderGenerator(model, symbolProvider, this, emptyList()).also { builderGen ->
        builderGen.render(writer)
        writer.implBlock(symbolProvider.toSymbol(this)) {
            builderGen.renderConvenienceMethod(this)
        }
    }
}
