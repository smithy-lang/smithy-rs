/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AnnotationTrait
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.getTrait

class SyntheticCustomShapeTrait(val ID: ShapeId, val symbol: Symbol) : AnnotationTrait(ID, Node.objectNode())

/**
 * This SymbolProvider honors [`SyntheticCustomShapeTrait`] and returns this trait's symbol when available.
 */
class CustomShapeSymbolProvider(private val base: RustSymbolProvider) : WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        return shape.getTrait<SyntheticCustomShapeTrait>()?.symbol ?: base.toSymbol(shape)
    }
}
