/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait

class StreamingShapeSymbolProvider(private val base: RustSymbolProvider, private val model: Model) : WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        val initial = base.toSymbol(shape)
        if (shape !is MemberShape) {
            return initial
        }
        val target = model.expectShape(shape.target)
        val container = model.expectShape(shape.container)
        if (!container.hasTrait(SyntheticOutputTrait::class.java)) {
            return initial
        }
        return if (target is BlobShape && target.hasTrait(StreamingTrait::class.java)) {
            val runtimeType = RuntimeType.byteStream(config().runtimeConfig)
            val rustType = RustType.Opaque(runtimeType.name!!, null, typeParameters = listOf("B"))
            runtimeType.toSymbol().toBuilder().rustType(rustType).build()
        } else {
            base.toSymbol(shape)
        }
    }
}
