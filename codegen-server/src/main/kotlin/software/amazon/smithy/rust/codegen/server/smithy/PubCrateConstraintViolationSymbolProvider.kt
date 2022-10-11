/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.rustType

/**
 * This is only used when `publicConstrainedTypes` is `false`.
 *
 * This must wrap [ConstraintViolationSymbolProvider].
 */
class PubCrateConstraintViolationSymbolProvider(
    private val base: ConstraintViolationSymbolProvider,
) : WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        val baseSymbol = base.toSymbol(shape)
        val baseRustType = baseSymbol.rustType()
        val newNamespace = baseSymbol.namespace + "_internal"
        return baseSymbol.toBuilder()
            .rustType(RustType.Opaque(baseRustType.name, newNamespace))
            .namespace(newNamespace, baseSymbol.namespaceDelimiter)
            .build()
    }
}
