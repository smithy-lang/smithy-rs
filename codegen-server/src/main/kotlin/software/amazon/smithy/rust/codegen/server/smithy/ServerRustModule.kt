/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.ModuleProvider
import software.amazon.smithy.rust.codegen.core.smithy.ModuleProviderContext
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

object ServerRustModule {
    val root = RustModule.LibRs

    val Error = RustModule.public("error")
    val Operation = RustModule.public("operation")
    val OperationShape = RustModule.public("operation_shape")
    val Model = RustModule.public("model")
    val Input = RustModule.public("input")
    val Output = RustModule.public("output")
    val Types = RustModule.public("types")
    val Server = RustModule.public("server")

    val UnconstrainedModule =
        software.amazon.smithy.rust.codegen.core.smithy.UnconstrainedModule
    val ConstrainedModule =
        software.amazon.smithy.rust.codegen.core.smithy.ConstrainedModule
}

class ServerModuleDocProvider : ModuleDocProvider {
    override fun docs(module: RustModule.LeafModule): String? = when (module) {
        ServerRustModule.Error -> "All error types that operations can return. Documentation on these types is copied from the model."
        ServerRustModule.Operation -> "All operations that this crate can perform."
        // TODO(ServerTeam): Document this module (I don't have context)
        ServerRustModule.OperationShape -> ""
        ServerRustModule.Model -> "Data structures used by operation inputs/outputs. Documentation on these types is copied from the model."
        ServerRustModule.Input -> "Input structures for operations. Documentation on these types is copied from the model."
        ServerRustModule.Output -> "Output structures for operations. Documentation on these types is copied from the model."
        ServerRustModule.Types -> "Data primitives referenced by other data types."
        ServerRustModule.Server -> "Contains the types that are re-exported from the `aws-smithy-http-server` crate."
        ServerRustModule.UnconstrainedModule -> "Unconstrained types for constrained shapes."
        ServerRustModule.ConstrainedModule -> "Constrained types for constrained shapes."
        else -> TODO("Document this module: $module")
    }
}

object ServerModuleProvider : ModuleProvider {
    override fun moduleForShape(context: ModuleProviderContext, shape: Shape): RustModule.LeafModule = when (shape) {
        is OperationShape -> ServerRustModule.Operation
        is StructureShape -> when {
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

    override fun moduleForBuilder(context: ModuleProviderContext, shape: Shape, symbol: Symbol): RustModule.LeafModule {
        val pubCrate = !(context.settings as ServerRustSettings).codegenConfig.publicConstrainedTypes
        val builderNamespace = RustReservedWords.escapeIfNeeded(symbol.name.toSnakeCase()) +
            if (pubCrate) {
                "_internal"
            } else {
                ""
            }
        val visibility = when (pubCrate) {
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
