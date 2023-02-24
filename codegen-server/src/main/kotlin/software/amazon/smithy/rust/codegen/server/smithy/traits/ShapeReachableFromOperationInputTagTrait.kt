/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.traits

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.AnnotationTrait
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.hasTrait

/**
 * Tag to indicate that an aggregate shape is reachable from operation input.
 *
 * See the [ShapesReachableFromOperationInputTagger] model transform for how it's used.
 */
class ShapeReachableFromOperationInputTagTrait : AnnotationTrait(ID, Node.objectNode()) {
    companion object {
        val ID = ShapeId.from("smithy.api.internal#syntheticStructureReachableFromOperationInputTag")
    }
}

private fun isShapeReachableFromOperationInput(shape: Shape) = when (shape) {
    is StructureShape, is UnionShape, is MapShape, is ListShape, is StringShape, is IntegerShape, is ShortShape, is LongShape, is ByteShape, is BlobShape -> {
        shape.hasTrait<ShapeReachableFromOperationInputTagTrait>()
    }

    else -> PANIC("this method does not support shape type ${shape.type}")
}

fun StringShape.isReachableFromOperationInput() = isShapeReachableFromOperationInput(this)
fun StructureShape.isReachableFromOperationInput() = isShapeReachableFromOperationInput(this)
fun CollectionShape.isReachableFromOperationInput() = isShapeReachableFromOperationInput(this)
fun UnionShape.isReachableFromOperationInput() = isShapeReachableFromOperationInput(this)
fun MapShape.isReachableFromOperationInput() = isShapeReachableFromOperationInput(this)
fun IntegerShape.isReachableFromOperationInput() = isShapeReachableFromOperationInput(this)
fun NumberShape.isReachableFromOperationInput() = isShapeReachableFromOperationInput(this)
fun BlobShape.isReachableFromOperationInput() = isShapeReachableFromOperationInput(this)
