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
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule

/**
 * Symbol provider with access to config and functions to get symbols for types that don't have shapes.
 */
interface RustSymbolProvider : SymbolProvider {
    val model: Model
    val config: SymbolVisitorConfig

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
    fun moduleForShape(
        shapeName: String,
        shape: Shape,
    ): RustModule.LeafModule

    /** Returns the module for an operation error */
    fun moduleForOperationError(
        operationName: String,
        operation: OperationShape,
    ): RustModule.LeafModule

    /** Returns the module for an event stream error */
    fun moduleForEventStreamError(
        eventStreamName: String,
        eventStream: UnionShape,
    ): RustModule.LeafModule
}

data class SymbolVisitorConfig(
    val runtimeConfig: RuntimeConfig,
    val renameExceptions: Boolean,
    val nullabilityCheckMode: NullableIndex.CheckMode,
    val moduleProvider: ModuleProvider,
)

/**
 * Delegator to enable easily decorating another named symbol provider.
 */
open class WrappingSymbolProvider(private val base: RustSymbolProvider) : RustSymbolProvider {
    override val model: Model get() = base.model
    override val config: SymbolVisitorConfig get() = base.config

    override fun symbolForOperationError(operation: OperationShape): Symbol = base.symbolForOperationError(operation)

    override fun symbolForEventStreamError(eventStream: UnionShape): Symbol = base.symbolForEventStreamError(eventStream)

    override fun toMemberName(shape: MemberShape): String = base.toMemberName(shape)

    override fun toSymbol(shape: Shape): Symbol = base.toSymbol(shape)
}
