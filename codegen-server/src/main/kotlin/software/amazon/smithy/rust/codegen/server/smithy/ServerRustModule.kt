/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.ModuleProvider
import software.amazon.smithy.rust.codegen.core.smithy.ModuleProviderContext
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.generators.DocHandlerGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.handlerImports

object ServerRustModule {
    val root = RustModule.LibRs

    val Error = RustModule.public("error")
    val Operation = RustModule.public("operation")
    val OperationShape = RustModule.public("operation_shape")
    val Model = RustModule.public("model")
    val Input = RustModule.public("input")
    val Output = RustModule.public("output")
    val Types = RustModule.public("types")
    val Service = RustModule.private("service")
    val Server = RustModule.public("server", inline = true)

    val UnconstrainedModule =
        software.amazon.smithy.rust.codegen.core.smithy.UnconstrainedModule
    val ConstrainedModule =
        software.amazon.smithy.rust.codegen.core.smithy.ConstrainedModule
}

class ServerModuleDocProvider(private val codegenContext: ServerCodegenContext) : ModuleDocProvider {
    override fun docsWriter(module: RustModule.LeafModule): Writable? {
        val strDoc: (String) -> Writable = { str -> writable { docs(escape(str)) } }
        return when (module) {
            ServerRustModule.Error -> strDoc("All error types that operations can return. Documentation on these types is copied from the model.")
            ServerRustModule.Operation -> strDoc("All operations that this crate can perform.")
            ServerRustModule.OperationShape -> operationShapeModuleDoc()
            ServerRustModule.Model -> strDoc("Data structures used by operation inputs/outputs. Documentation on these types is copied from the model.")
            ServerRustModule.Input -> strDoc("Input structures for operations. Documentation on these types is copied from the model.")
            ServerRustModule.Output -> strDoc("Output structures for operations. Documentation on these types is copied from the model.")
            ServerRustModule.Types -> strDoc("Data primitives referenced by other data types.")
            ServerRustModule.Server -> strDoc("Contains the types that are re-exported from the `aws-smithy-http-server` crate.")
            ServerRustModule.UnconstrainedModule -> strDoc("Unconstrained types for constrained shapes.")
            ServerRustModule.ConstrainedModule -> strDoc("Constrained types for constrained shapes.")
            else -> TODO("Document this module: $module")
        }
    }

    private fun operationShapeModuleDoc(): Writable =
        writable {
            val index = TopDownIndex.of(codegenContext.model)
            val operations = index.getContainedOperations(codegenContext.serviceShape).toSortedSet(compareBy { it.id })

            val firstOperation = operations.first() ?: return@writable
            val crateName = codegenContext.settings.moduleName.toSnakeCase()

            rustTemplate(
                """
                /// A collection of types representing each operation defined in the service closure.
                ///
                /// The [plugin system](#{SmithyHttpServer}::plugin) makes use of these
                /// [zero-sized types](https://doc.rust-lang.org/nomicon/exotic-sizes.html##zero-sized-types-zsts) (ZSTs) to
                /// parameterize [`Plugin`](#{SmithyHttpServer}::plugin::Plugin) implementations. Their traits, such as
                /// [`OperationShape`](#{SmithyHttpServer}::operation::OperationShape), can be used to provide
                /// operation specific information to the [`Layer`](#{Tower}::Layer) being applied.
                """.trimIndent(),
                "SmithyHttpServer" to
                    ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                "Tower" to ServerCargoDependency.Tower.toType(),
                "Handler" to DocHandlerGenerator(codegenContext, firstOperation, "handler", commentToken = "///").docSignature(),
                "HandlerImports" to handlerImports(crateName, operations, commentToken = "///"),
            )
        }
}

object ServerModuleProvider : ModuleProvider {
    override fun moduleForShape(
        context: ModuleProviderContext,
        shape: Shape,
    ): RustModule.LeafModule =
        when (shape) {
            is OperationShape -> ServerRustModule.Operation
            is StructureShape ->
                when {
                    shape.hasTrait<ErrorTrait>() -> ServerRustModule.Error
                    shape.hasTrait<SyntheticInputTrait>() -> ServerRustModule.Input
                    shape.hasTrait<SyntheticOutputTrait>() -> ServerRustModule.Output
                    else -> ServerRustModule.Model
                }
            else -> ServerRustModule.Model
        }

    override fun moduleForOperationError(
        context: ModuleProviderContext,
        operation: OperationShape,
    ): RustModule.LeafModule = ServerRustModule.Error

    override fun moduleForEventStreamError(
        context: ModuleProviderContext,
        eventStream: UnionShape,
    ): RustModule.LeafModule = ServerRustModule.Error

    override fun moduleForBuilder(
        context: ModuleProviderContext,
        shape: Shape,
        symbol: Symbol,
    ): RustModule.LeafModule {
        val pubCrate = !(context.settings as ServerRustSettings).codegenConfig.publicConstrainedTypes
        val builderNamespace =
            RustReservedWords.escapeIfNeeded(symbol.name.toSnakeCase()) +
                if (pubCrate) {
                    "_internal"
                } else {
                    ""
                }
        val visibility =
            when (pubCrate) {
                true -> Visibility.PUBCRATE
                false -> Visibility.PUBLIC
            }
        return RustModule.new(
            builderNamespace,
            visibility,
            parent = symbol.module(),
            inline = true,
            documentationOverride = "",
        )
    }
}
