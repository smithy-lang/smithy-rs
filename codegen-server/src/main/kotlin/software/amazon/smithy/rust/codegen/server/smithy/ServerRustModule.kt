/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.smithy.ModuleProvider
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait

object ServerRustModule {
    val ErrorsModule = RustModule.public("error", documentation = "All error types that operations can return. Documentation on these types is copied from the model.")
    val OperationsModule = RustModule.public("operation", documentation = "All operations that this crate can perform.")
    val ModelsModule = RustModule.public("model", documentation = "Data structures used by operation inputs/outputs. Documentation on these types is copied from the model.")
    val InputsModule = RustModule.public("input", documentation = "Input structures for operations. Documentation on these types is copied from the model.")
    val OutputsModule = RustModule.public("output", documentation = "Output structures for operations. Documentation on these types is copied from the model.")

    val UnconstrainedModule =
        software.amazon.smithy.rust.codegen.core.smithy.UnconstrainedModule
    val ConstrainedModule =
        software.amazon.smithy.rust.codegen.core.smithy.ConstrainedModule
}

object ServerModuleProvider : ModuleProvider {
    override fun moduleForShape(shape: Shape): RustModule.LeafModule = when (shape) {
        is OperationShape -> ServerRustModule.OperationsModule
        is StructureShape -> when {
            shape.hasTrait<ErrorTrait>() -> ServerRustModule.ErrorsModule
            shape.hasTrait<SyntheticInputTrait>() -> ServerRustModule.InputsModule
            shape.hasTrait<SyntheticOutputTrait>() -> ServerRustModule.OutputsModule
            else -> ServerRustModule.ModelsModule
        }
        else -> ServerRustModule.ModelsModule
    }

    override fun moduleForOperationError(operation: OperationShape): RustModule.LeafModule =
        ServerRustModule.ErrorsModule

    override fun moduleForEventStreamError(eventStream: UnionShape): RustModule.LeafModule =
        ServerRustModule.ErrorsModule
}
