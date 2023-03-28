/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule

/**
 * SymbolProvider interface that carries additional configuration and module/symbol resolution.
 */
interface RustSymbolProvider : SymbolProvider {
    val model: Model
    val moduleProviderContext: ModuleProviderContext
    val config: RustSymbolProviderConfig

    fun moduleForShape(shape: Shape): RustModule.LeafModule =
        config.moduleProvider.moduleForShape(moduleProviderContext, shape)
    fun moduleForOperationError(operation: OperationShape): RustModule.LeafModule =
        config.moduleProvider.moduleForOperationError(moduleProviderContext, operation)
    fun moduleForEventStreamError(eventStream: UnionShape): RustModule.LeafModule =
        config.moduleProvider.moduleForEventStreamError(moduleProviderContext, eventStream)
    fun moduleForBuilder(shape: Shape): RustModule.LeafModule =
        config.moduleProvider.moduleForBuilder(moduleProviderContext, shape, toSymbol(shape))

    /** Returns the symbol for an operation error */
    fun symbolForOperationError(operation: OperationShape): Symbol

    /** Returns the symbol for an event stream error */
    fun symbolForEventStreamError(eventStream: UnionShape): Symbol

    /** Returns the symbol for a builder */
    fun symbolForBuilder(shape: Shape): Symbol
}

/**
 * Module providers can't use the full CodegenContext since they're invoked from
 * inside the SymbolVisitor, which is created before CodegenContext is created.
 */
data class ModuleProviderContext(
    val settings: CoreRustSettings,
    val model: Model,
    val serviceShape: ServiceShape?,
)

fun CodegenContext.toModuleProviderContext(): ModuleProviderContext =
    ModuleProviderContext(settings, model, serviceShape)

/**
 * Provider for RustModules so that the symbol provider knows where to organize things.
 */
interface ModuleProvider {
    /** Returns the module for a shape */
    fun moduleForShape(context: ModuleProviderContext, shape: Shape): RustModule.LeafModule

    /** Returns the module for an operation error */
    fun moduleForOperationError(context: ModuleProviderContext, operation: OperationShape): RustModule.LeafModule

    /** Returns the module for an event stream error */
    fun moduleForEventStreamError(context: ModuleProviderContext, eventStream: UnionShape): RustModule.LeafModule

    /** Returns the module for a builder */
    fun moduleForBuilder(context: ModuleProviderContext, shape: Shape, symbol: Symbol): RustModule.LeafModule
}

/**
 * Configuration for symbol providers.
 */
data class RustSymbolProviderConfig(
    val runtimeConfig: RuntimeConfig,
    val renameExceptions: Boolean,
    val nullabilityCheckMode: NullableIndex.CheckMode,
    val moduleProvider: ModuleProvider,
    val nameBuilderFor: (Symbol) -> String = { _ -> "Builder" },
)

/**
 * Default delegator to enable easily decorating another symbol provider.
 */
open class WrappingSymbolProvider(private val base: RustSymbolProvider) : RustSymbolProvider {
    override val model: Model get() = base.model
    override val moduleProviderContext: ModuleProviderContext get() = base.moduleProviderContext
    override val config: RustSymbolProviderConfig get() = base.config

    override fun toSymbol(shape: Shape): Symbol = base.toSymbol(shape)
    override fun toMemberName(shape: MemberShape): String = base.toMemberName(shape)
    override fun symbolForOperationError(operation: OperationShape): Symbol = base.symbolForOperationError(operation)
    override fun symbolForEventStreamError(eventStream: UnionShape): Symbol =
        base.symbolForEventStreamError(eventStream)
    override fun symbolForBuilder(shape: Shape): Symbol = base.symbolForBuilder(shape)
}
