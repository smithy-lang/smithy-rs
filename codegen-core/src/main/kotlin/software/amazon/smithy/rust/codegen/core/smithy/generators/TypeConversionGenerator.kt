/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.rustType

/*
 * Utility class used to force casting a non primitive type into one overriden by a new symbol provider,
 * by explicitly calling `from()` or into().
 *
 * For example we use this in the server Python implementation, where we override types like [Blob] and [DateTime]
 * with wrappers compatible with Python, without touching the original implementation coming from `aws-smithy-types`.
 */
class TypeConversionGenerator(private val model: Model, private val symbolProvider: RustSymbolProvider, private val runtimeConfig: RuntimeConfig) {
    private fun findOldSymbol(shape: Shape): Symbol {
        return when (shape) {
            is BlobShape -> RuntimeType.blob(runtimeConfig).toSymbol()
            is TimestampShape -> RuntimeType.dateTime(runtimeConfig).toSymbol()
            else -> symbolProvider.toSymbol(shape)
        }
    }

    fun convertViaFrom(shape: Shape): Writable =
        writable {
            val oldSymbol = findOldSymbol(shape)
            val newSymbol = symbolProvider.toSymbol(shape)
            if (oldSymbol.rustType() != newSymbol.rustType()) {
                rust(".map($newSymbol::from)")
            }
        }

    fun convertViaInto(shape: Shape): Writable =
        writable {
            val oldSymbol = findOldSymbol(shape)
            val newSymbol = symbolProvider.toSymbol(shape)
            if (oldSymbol.rustType() != newSymbol.rustType()) {
                rust(".into()")
            }
        }
}
