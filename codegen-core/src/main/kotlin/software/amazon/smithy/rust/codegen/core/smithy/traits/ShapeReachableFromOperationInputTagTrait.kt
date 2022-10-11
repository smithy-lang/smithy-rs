/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.traits

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.AnnotationTrait
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.hasTrait

/**
 * Tag to indicate that an aggregate shape is reachable from operation input.
 *
 * See the [AggregateShapesReachableFromOperationInputTagger] model transform for how it's used.
 */
class ShapeReachableFromOperationInputTagTrait() : AnnotationTrait(ID, Node.objectNode()) {
    companion object {
        val ID = ShapeId.from("smithy.api.internal#syntheticStructureReachableFromOperationInputTag")
    }
}

private fun isShapeReachableFromOperationInput(shape: Shape) = when (shape) {
    is StructureShape, is UnionShape, is ListShape, is MapShape -> {
        // TODO There are a bunch of sites where we're performing this check inline instead of calling this function.
        shape.hasTrait<ShapeReachableFromOperationInputTagTrait>()
    } else -> PANIC("this method does not support shape type ${shape.type}")
}

fun StructureShape.isReachableFromOperationInput() = isShapeReachableFromOperationInput(this)
fun CollectionShape.isReachableFromOperationInput() = isShapeReachableFromOperationInput(this)
fun UnionShape.isReachableFromOperationInput() = isShapeReachableFromOperationInput(this)
fun MapShape.isReachableFromOperationInput() = isShapeReachableFromOperationInput(this)
