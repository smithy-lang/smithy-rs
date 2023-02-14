/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.util.letIf

/**
 * Attaches modules to symbols.
 *
 * This happens separately at the end of the symbol provider chain so that the result of
 * the RustReservedWordSymbolProvider can impact module names.
 */
class ModuleAttachingSymbolProvider(base: RustSymbolProvider) : WrappingSymbolProvider(base) {
    override fun symbolForOperationError(operation: OperationShape): Symbol =
        super.symbolForOperationError(operation).let { symbol ->
            val module = config.moduleProvider.moduleForOperationError(symbol.name, operation)
            symbol.toBuilder().locatedIn(module).build()
        }

    override fun symbolForEventStreamError(eventStream: UnionShape): Symbol =
        super.symbolForEventStreamError(eventStream).let { symbol ->
            val module = config.moduleProvider.moduleForEventStreamError(symbol.name, eventStream)
            symbol.toBuilder().locatedIn(module).build()
        }

    override fun toSymbol(shape: Shape): Symbol = super.toSymbol(shape).let { symbol ->
        val module = config.moduleProvider.moduleForShape(symbol.name, shape)
        val newRustType = when (val oldRustType = symbol.rustType()) {
            is RustType.Container -> {
                val maybeNewType: RustType = oldRustType.recursivelyMapMember { member ->
                    if (member is RustType.Opaque && member.namespace == null) {
                        member.copy(namespace = module.fullyQualifiedPath())
                    } else {
                        member
                    }
                }
                (maybeNewType as RustType?).letIf(maybeNewType == oldRustType) { null }
            }
            is RustType.Opaque -> if (oldRustType.namespace == null) {
                oldRustType.copy(namespace = module.fullyQualifiedPath())
            } else {
                null
            }
            // Other RustType variants (which are mostly std library types) have modules that shouldn't be manipulated
            else -> null
        }
        if (newRustType != null) {
            symbol.toBuilder().definitionFile(module.definitionFile())
                .namespace(module.fullyQualifiedPath(), "::")
                .module(module)
                .rustType(newRustType)
                .build()
        } else {
            symbol
        }
    }
}
