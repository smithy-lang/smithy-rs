/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.server.smithy.traits.ShapeReachableFromOperationInputTagTrait

/**
 * Tag shapes reachable from operation input with the
 * [ShapeReachableFromOperationInputTagTrait] tag.
 *
 * This is useful to determine whether we need to generate code to
 * enforce constraints upon request deserialization in the server.
 *
 * This needs to be a model transformer; it cannot be lazily calculated
 * when needed. This is because other model transformers may transform
 * the model such that shapes that were reachable from operation
 * input are no longer so. For example, [EventStreamNormalizer] pulls
 * event stream error variants out of the union shape where they are defined.
 * As such, [ShapesReachableFromOperationInputTagger] needs to run
 * before these model transformers.
 *
 * WARNING: This transformer tags _all_ [aggregate shapes], and _some_ [simple shapes],
 * but not all of them. Read the implementation to find out what shape types it
 * currently tags.
 *
 * [simple shapes]: https://awslabs.github.io/smithy/2.0/spec/simple-types.html
 * [aggregate shapes]: https://awslabs.github.io/smithy/2.0/spec/aggregate-types.html#aggregate-types
 */
object ShapesReachableFromOperationInputTagger {
    fun transform(model: Model): Model {
        val inputShapes = model.operationShapes.map {
            model.expectShape(it.inputShape, StructureShape::class.java)
        }
        val walker = DirectedWalker(model)
        val shapesReachableFromOperationInputs = inputShapes
            .flatMap { walker.walkShapes(it) }
            .toSet()

        return ModelTransformer.create().mapShapes(model) { shape ->
            when (shape) {
                is StructureShape, is UnionShape, is ListShape, is MapShape, is StringShape, is IntegerShape, is ShortShape, is LongShape, is ByteShape, is BlobShape -> {
                    if (shapesReachableFromOperationInputs.contains(shape)) {
                        val builder = when (shape) {
                            is StructureShape -> shape.toBuilder()
                            is UnionShape -> shape.toBuilder()
                            is ListShape -> shape.toBuilder()
                            is MapShape -> shape.toBuilder()
                            is StringShape -> shape.toBuilder()
                            is IntegerShape -> shape.toBuilder()
                            is ShortShape -> shape.toBuilder()
                            is LongShape -> shape.toBuilder()
                            is ByteShape -> shape.toBuilder()
                            is BlobShape -> shape.toBuilder()
                            else -> UNREACHABLE("the `when` is exhaustive")
                        }
                        builder.addTrait(ShapeReachableFromOperationInputTagTrait()).build()
                    } else {
                        shape
                    }
                }
                else -> shape
            }
        }
    }
}
