/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType

/**
 * Rewrite crate::* with `<moduleusename>::*`
 *
 * This enables generating code into `tests` and other public locations.
 */
class PublicImportSymbolProvider(private val base: RustSymbolProvider, private val publicUseName: String) :
    WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        val baseSymbol = base.toSymbol(shape)

        val currentRustType = baseSymbol.rustType()
        val currentNamespace = currentRustType.namespace
        val newRustType =
            if (currentRustType is RustType.Opaque && currentNamespace != null && currentNamespace.startsWith("crate::")) {
                currentRustType.copy(namespace = currentNamespace.replace("crate::", "$publicUseName::"))
            } else {
                currentRustType
            }
        return baseSymbol.toBuilder().rustType(newRustType).build()
    }
}
