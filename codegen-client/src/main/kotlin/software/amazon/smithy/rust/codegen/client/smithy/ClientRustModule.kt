/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

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

/**
 * Modules for code generated client crates.
 */
object ClientRustModule {
    /** crate */
    val root = RustModule.LibRs

    /** crate::client */
    val client = Client.self
    object Client {
        /** crate::client */
        val self = RustModule.public("client", "Client and fluent builders for calling the service.")

        /** crate::client::customize */
        val customize = RustModule.public("customize", "Operation customization and supporting types", parent = self)
    }

    val Config = RustModule.public("config", documentation = "Configuration for the service.")
    val Error = RustModule.public("error", documentation = "All error types that operations can return. Documentation on these types is copied from the model.")
    val Operation = RustModule.public("operation", documentation = "All operations that this crate can perform.")
    val Model = RustModule.public("model", documentation = "Data structures used by operation inputs/outputs. Documentation on these types is copied from the model.")
    val Input = RustModule.public("input", documentation = "Input structures for operations. Documentation on these types is copied from the model.")
    val Output = RustModule.public("output", documentation = "Output structures for operations. Documentation on these types is copied from the model.")
    val Types = RustModule.public("types", documentation = "Data primitives referenced by other data types.")
}

object ClientModuleProvider : ModuleProvider {
    override fun moduleForShape(shape: Shape): RustModule.LeafModule = when (shape) {
        is OperationShape -> ClientRustModule.Operation
        is StructureShape -> when {
            shape.hasTrait<ErrorTrait>() -> ClientRustModule.Error
            shape.hasTrait<SyntheticInputTrait>() -> ClientRustModule.Input
            shape.hasTrait<SyntheticOutputTrait>() -> ClientRustModule.Output
            else -> ClientRustModule.Model
        }
        else -> ClientRustModule.Model
    }

    override fun moduleForOperationError(operation: OperationShape): RustModule.LeafModule =
        ClientRustModule.Error

    override fun moduleForEventStreamError(eventStream: UnionShape): RustModule.LeafModule =
        ClientRustModule.Error
}

// TODO(CrateReorganization): Remove when cleaning up `enableNewCrateOrganizationScheme`
fun ClientCodegenContext.featureGatedConfigModule() = when (settings.codegenConfig.enableNewCrateOrganizationScheme) {
    true -> ClientRustModule.Config
    else -> ClientRustModule.root
}
