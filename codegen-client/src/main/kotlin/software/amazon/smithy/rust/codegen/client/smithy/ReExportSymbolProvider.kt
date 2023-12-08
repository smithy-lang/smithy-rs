/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.runtimeType

class ReExportSymbolProvider(private val base: RustSymbolProvider) : WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        val baseSymbol = base.toSymbol(shape)
        // We are only targeting member shapes
        if (shape !is MemberShape) {
            return baseSymbol
        }
        return when (base.model.expectShape(shape.target)) {
            is BlobShape, is TimestampShape -> {
                if (!baseSymbol.properties.containsKey("runtimetype")) {
                    println(baseSymbol.properties)
                    TODO()
                }
                primitivesReexport(baseSymbol.runtimeType()).setSymbol(baseSymbol.toBuilder()).build()
            }
            else -> baseSymbol
        }
    }
}
