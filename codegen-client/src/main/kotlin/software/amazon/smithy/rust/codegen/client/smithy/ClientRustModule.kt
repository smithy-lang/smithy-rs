/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.smithy.ModuleProvider
import software.amazon.smithy.rust.codegen.core.smithy.ModuleProviderContext
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

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
    val Meta = RustModule.public("meta", documentation = "Information about this crate.")
    val Model = RustModule.public("model", documentation = "Data structures used by operation inputs/outputs. Documentation on these types is copied from the model.")
    val Input = RustModule.public("input", documentation = "Input structures for operations. Documentation on these types is copied from the model.")
    val Output = RustModule.public("output", documentation = "Output structures for operations. Documentation on these types is copied from the model.")
    val Types = RustModule.public("types", documentation = "Data primitives referenced by other data types.")
}

object ClientModuleProvider : ModuleProvider {
    override fun moduleForShape(context: ModuleProviderContext, shape: Shape): RustModule.LeafModule = when (shape) {
        is OperationShape -> perOperationModule(context, shape)
        is StructureShape -> when {
            shape.hasTrait<ErrorTrait>() -> ClientRustModule.Error
            shape.hasTrait<SyntheticInputTrait>() -> perOperationModule(context, shape)
            shape.hasTrait<SyntheticOutputTrait>() -> perOperationModule(context, shape)
            else -> ClientRustModule.Model
        }

        else -> ClientRustModule.Model
    }

    override fun moduleForOperationError(
        context: ModuleProviderContext,
        operation: OperationShape,
    ): RustModule.LeafModule = perOperationModule(context, operation)

    override fun moduleForEventStreamError(
        context: ModuleProviderContext,
        eventStream: UnionShape,
    ): RustModule.LeafModule = ClientRustModule.Error

    private fun Shape.findOperation(model: Model): OperationShape {
        val inputTrait = getTrait<SyntheticInputTrait>()
        val outputTrait = getTrait<SyntheticOutputTrait>()
        return when {
            this is OperationShape -> this
            inputTrait != null -> model.expectShape(inputTrait.operation, OperationShape::class.java)
            outputTrait != null -> model.expectShape(outputTrait.operation, OperationShape::class.java)
            else -> UNREACHABLE("this is only called with compatible shapes")
        }
    }

    private fun perOperationModule(context: ModuleProviderContext, shape: Shape): RustModule.LeafModule {
        val operationShape = shape.findOperation(context.model)
        val contextName = operationShape.contextName(context.serviceShape)
        val operationModuleName =
            RustReservedWords.escapeIfNeeded(contextName.toSnakeCase())
        return RustModule.public(
            operationModuleName,
            parent = ClientRustModule.Operation,
            documentation = "Types for the `$contextName` operation.",
        )
    }
}

// TODO(CrateReorganization): Remove this provider
object OldModuleSchemeClientModuleProvider : ModuleProvider {
    override fun moduleForShape(context: ModuleProviderContext, shape: Shape): RustModule.LeafModule = when (shape) {
        is OperationShape -> ClientRustModule.Operation
        is StructureShape -> when {
            shape.hasTrait<ErrorTrait>() -> ClientRustModule.Error
            shape.hasTrait<SyntheticInputTrait>() -> ClientRustModule.Input
            shape.hasTrait<SyntheticOutputTrait>() -> ClientRustModule.Output
            else -> ClientRustModule.Model
        }

        else -> ClientRustModule.Model
    }

    override fun moduleForOperationError(
        context: ModuleProviderContext,
        operation: OperationShape,
    ): RustModule.LeafModule = ClientRustModule.Error

    override fun moduleForEventStreamError(
        context: ModuleProviderContext,
        eventStream: UnionShape,
    ): RustModule.LeafModule = ClientRustModule.Error
}

// TODO(CrateReorganization): Remove when cleaning up `enableNewCrateOrganizationScheme`
fun ClientCodegenContext.featureGatedConfigModule() = when (settings.codegenConfig.enableNewCrateOrganizationScheme) {
    true -> ClientRustModule.Config
    else -> ClientRustModule.root
}

// TODO(CrateReorganization): Remove when cleaning up `enableNewCrateOrganizationScheme`
fun ClientCodegenContext.featureGatedCustomizeModule() = when (settings.codegenConfig.enableNewCrateOrganizationScheme) {
    true -> ClientRustModule.Client.customize
    else -> RustModule.public(
        "customize",
        "Operation customization and supporting types",
        parent = ClientRustModule.Operation,
    )
}

// TODO(CrateReorganization): Remove when cleaning up `enableNewCrateOrganizationScheme`
fun ClientCodegenContext.featureGatedMetaModule() = when (settings.codegenConfig.enableNewCrateOrganizationScheme) {
    true -> ClientRustModule.Meta
    else -> ClientRustModule.root
}

// TODO(CrateReorganization): Remove when cleaning up `enableNewCrateOrganizationScheme`
fun ClientCodegenContext.featureGatedPaginatorModule(symbolProvider: RustSymbolProvider, operation: OperationShape) =
    when (settings.codegenConfig.enableNewCrateOrganizationScheme) {
        true -> RustModule.public(
            "paginator",
            parent = symbolProvider.moduleForShape(operation),
            documentation = "Paginator for this operation",
        )
        else -> RustModule.public("paginator", "Paginators for the service")
    }
