/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule

/**
 * SymbolProvider interface that carries both the inner configuration and a function to produce an enum variant name.
 */
interface RustSymbolProvider : SymbolProvider, ModuleProvider {
    fun config(): RustSymbolProviderConfig
    fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed?

    override fun moduleForShape(shape: Shape): RustModule.LeafModule = config().moduleProvider.moduleForShape(shape)
    override fun moduleForOperationError(operation: OperationShape): RustModule.LeafModule =
        config().moduleProvider.moduleForOperationError(operation)
    override fun moduleForEventStreamError(eventStream: UnionShape): RustModule.LeafModule =
        config().moduleProvider.moduleForEventStreamError(eventStream)

    /** Returns the symbol for an operation error */
    fun symbolForOperationError(operation: OperationShape): Symbol

    /** Returns the symbol for an event stream error */
    fun symbolForEventStreamError(eventStream: UnionShape): Symbol
}

/**
 * Provider for RustModules so that the symbol provider knows where to organize things.
 */
interface ModuleProvider {
    /** Returns the module for a shape */
    fun moduleForShape(shape: Shape): RustModule.LeafModule

    /** Returns the module for an operation error */
    fun moduleForOperationError(operation: OperationShape): RustModule.LeafModule

    /** Returns the module for an event stream error */
    fun moduleForEventStreamError(eventStream: UnionShape): RustModule.LeafModule
}

/**
 * Configuration for symbol providers.
 */
data class RustSymbolProviderConfig(
    val runtimeConfig: RuntimeConfig,
    val renameExceptions: Boolean,
    val nullabilityCheckMode: NullableIndex.CheckMode,
    val moduleProvider: ModuleProvider,
)

/**
 * Default delegator to enable easily decorating another symbol provider.
 */
open class WrappingSymbolProvider(private val base: RustSymbolProvider) : RustSymbolProvider {
    override fun config(): RustSymbolProviderConfig = base.config()
    override fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed? = base.toEnumVariantName(definition)
    override fun toSymbol(shape: Shape): Symbol = base.toSymbol(shape)
    override fun toMemberName(shape: MemberShape): String = base.toMemberName(shape)
    override fun symbolForOperationError(operation: OperationShape): Symbol = base.symbolForOperationError(operation)
    override fun symbolForEventStreamError(eventStream: UnionShape): Symbol =
        base.symbolForEventStreamError(eventStream)
}
