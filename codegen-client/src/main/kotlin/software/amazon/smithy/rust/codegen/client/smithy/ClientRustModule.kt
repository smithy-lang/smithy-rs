/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
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
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.PANIC
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
        val self = RustModule.public("client")

        /** crate::client::customize */
        val customize = RustModule.public("customize", parent = self)
    }

    val Config = RustModule.public("config")
    val Error = RustModule.public("error")
    val Endpoint = RustModule.public("endpoint")
    val Operation = RustModule.public("operation")
    val Meta = RustModule.public("meta")
    val Input = RustModule.public("input")
    val Output = RustModule.public("output")
    val Primitives = RustModule.public("primitives")

    /** crate::types */
    val types = Types.self
    object Types {
        /** crate::types */
        val self = RustModule.public("types")

        /** crate::types::error */
        val Error = RustModule.public("error", parent = self)
    }

    // TODO(CrateReorganization): Remove this module when cleaning up `enableNewCrateOrganizationScheme`
    val Model = RustModule.public("model")
}

class ClientModuleDocProvider(
    private val config: ClientCodegenConfig,
    private val serviceName: String,
) : ModuleDocProvider {
    override fun docs(module: RustModule.LeafModule): String? =
        when (config.enableNewCrateOrganizationScheme) {
            true -> when (module) {
                ClientRustModule.client -> "Client for calling $serviceName."
                ClientRustModule.Client.customize -> "Operation customization and supporting types."
                ClientRustModule.Config -> "Configuration for $serviceName."
                ClientRustModule.Error -> "Common errors and error handling utilities."
                ClientRustModule.Endpoint -> "Endpoint resolution functionality."
                ClientRustModule.Operation -> "All operations that this crate can perform."
                ClientRustModule.Meta -> "Information about this crate."
                ClientRustModule.Input -> PANIC("this module shouldn't exist in the new scheme")
                ClientRustModule.Output -> PANIC("this module shouldn't exist in the new scheme")
                ClientRustModule.Primitives -> "Primitives such as `Blob` or `DateTime` used by other types."
                ClientRustModule.types -> "Data primitives referenced by other data types."
                ClientRustModule.Types.Error -> "Error types that $serviceName can respond with."
                ClientRustModule.Model -> PANIC("this module shouldn't exist in the new scheme")
                else -> TODO("Document this module: $module")
            }
            else -> when (module) {
                ClientRustModule.client -> "Client and fluent builders for calling $serviceName."
                ClientRustModule.Client.customize -> "Operation customization and supporting types."
                ClientRustModule.Config -> "Configuration for $serviceName."
                ClientRustModule.Error -> "All error types that operations can return. Documentation on these types is copied from the model."
                ClientRustModule.Endpoint -> "Endpoint resolution functionality."
                ClientRustModule.Operation -> "All operations that this crate can perform."
                ClientRustModule.Meta -> PANIC("this module shouldn't exist in the old scheme")
                ClientRustModule.Input -> "Input structures for operations. Documentation on these types is copied from the model."
                ClientRustModule.Output -> "Output structures for operations. Documentation on these types is copied from the model."
                ClientRustModule.Primitives -> PANIC("this module shouldn't exist in the old scheme")
                ClientRustModule.types -> "Data primitives referenced by other data types."
                ClientRustModule.Types.Error -> PANIC("this module shouldn't exist in the old scheme")
                ClientRustModule.Model -> "Data structures used by operation inputs/outputs."
                else -> TODO("Document this module: $module")
            }
        }
}

object ClientModuleProvider : ModuleProvider {
    override fun moduleForShape(context: ModuleProviderContext, shape: Shape): RustModule.LeafModule = when (shape) {
        is OperationShape -> perOperationModule(context, shape)
        is StructureShape -> when {
            shape.hasTrait<ErrorTrait>() -> ClientRustModule.Types.Error
            shape.hasTrait<SyntheticInputTrait>() -> perOperationModule(context, shape)
            shape.hasTrait<SyntheticOutputTrait>() -> perOperationModule(context, shape)
            else -> ClientRustModule.types
        }

        else -> ClientRustModule.types
    }

    override fun moduleForOperationError(
        context: ModuleProviderContext,
        operation: OperationShape,
    ): RustModule.LeafModule = perOperationModule(context, operation)

    override fun moduleForEventStreamError(
        context: ModuleProviderContext,
        eventStream: UnionShape,
    ): RustModule.LeafModule = ClientRustModule.Types.Error

    override fun moduleForBuilder(context: ModuleProviderContext, shape: Shape, symbol: Symbol): RustModule.LeafModule =
        RustModule.public("builders", parent = symbol.module(), documentationOverride = "Builders")

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
            documentationOverride = "Types for the `$contextName` operation.",
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

    override fun moduleForBuilder(context: ModuleProviderContext, shape: Shape, symbol: Symbol): RustModule.LeafModule {
        val builderNamespace = RustReservedWords.escapeIfNeeded(symbol.name.toSnakeCase())
        return RustModule.new(
            builderNamespace,
            visibility = Visibility.PUBLIC,
            parent = symbol.module(),
            inline = true,
            documentationOverride = "See [`${symbol.name}`](${symbol.module().fullyQualifiedPath()}::${symbol.name}).",
        )
    }
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
        parent = ClientRustModule.Operation,
        documentationOverride = "Operation customization and supporting types",
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
            documentationOverride = "Paginator for this operation",
        )
        else -> RustModule.public("paginator", documentationOverride = "Paginators for the service")
    }

// TODO(CrateReorganization): Remove when cleaning up `enableNewCrateOrganizationScheme`
fun ClientCodegenContext.featureGatedPrimitivesModule() = when (settings.codegenConfig.enableNewCrateOrganizationScheme) {
    true -> ClientRustModule.Primitives
    else -> ClientRustModule.types
}
