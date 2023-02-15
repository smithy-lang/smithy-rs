/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.transformers

import software.amazon.smithy.codegen.core.TopologicalIndex
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.smithy.traits.RustBoxTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait

class RecursiveShapeBoxer(
    /**
     * A predicate that determines when a cycle in the shape graph contains "indirection". If a cycle contains
     * indirection, no shape needs to be tagged. What constitutes indirection is up to the caller to decide.
     */
    private val containsIndirectionPredicate: (Collection<Shape>) -> Boolean = ::containsIndirection,
    /**
     * A closure that gets called on one member shape of a cycle that does not contain indirection for "fixing". For
     * example, the [RustBoxTrait] trait can be used to tag the member shape.
     */
    private val boxShapeFn: (MemberShape) -> MemberShape = ::addRustBoxTrait,
) {
    /**
     * Transform a model which may contain recursive shapes.
     *
     * For example, when recursive shapes do NOT go through a `CollectionShape` or a `MapShape` shape, they must be
     * boxed in Rust. This function will iteratively find cycles and call [boxShapeFn] on a member shape in the
     * cycle to act on it. This is done in a deterministic way until it reaches a fixed point.
     *
     * This function MUST be deterministic (always choose the same shapes to fix). If it is not, that is a bug. Even so
     * this function may cause backward compatibility issues in certain pathological cases where a changes to recursive
     * structures cause different members to be boxed. We may need to address these via customizations.
     *
     * For example, given the following model,
     *
     * ```smithy
     * namespace com.example
     *
     * structure Recursive {
     *     recursiveStruct: Recursive
     *     anotherField: Boolean
     * }
     * ```
     *
     * The `com.example#Recursive$recursiveStruct` member shape is part of a cycle, but the
     * `com.example#Recursive$anotherField` member shape is not.
     */
    fun transform(model: Model): Model {
        val next = transformInner(model)
        return if (next == null) {
            model
        } else {
            transform(next)
        }
    }

    /**
     * If [model] contains a recursive loop that must be boxed, return the transformed model resulting form a call to
     * [boxShapeFn].
     * If [model] contains no loops, return `null`.
     */
    private fun transformInner(model: Model): Model? {
        // Execute 1 step of the boxing algorithm in the path to reaching a fixed point:
        // 1. Find all the shapes that are part of a cycle.
        // 2. Find all the loops that those shapes are part of.
        // 3. Filter out the loops that go through a layer of indirection.
        // 3. Pick _just one_ of the remaining loops to fix.
        // 4. Select the member shape in that loop with the earliest shape id.
        // 5. Box it.
        // (External to this function) Go back to 1.
        val index = TopologicalIndex.of(model)
        val recursiveShapes = index.recursiveShapes
        val loops = recursiveShapes.map { shapeId ->
            // Get all the shapes in the closure (represented as `Path`s).
            index.getRecursiveClosure(shapeId)
        }.flatMap { loops ->
            // Flatten the connections into shapes.
            loops.map { it.shapes }
        }
        val loopToFix = loops.firstOrNull { !containsIndirectionPredicate(it) }

        return loopToFix?.let { loop: List<Shape> ->
            check(loop.isNotEmpty())
            // Pick the shape to box in a deterministic way.
            val shapeToBox = loop.filterIsInstance<MemberShape>().minByOrNull { it.id }!!
            ModelTransformer.create().mapShapes(model) { shape ->
                if (shape == shapeToBox) {
                    boxShapeFn(shape.asMemberShape().get())
                } else {
                    shape
                }
            }
        }
    }
}

/**
 * Check if a `List<Shape>` contains a shape which will use a pointer when represented in Rust, avoiding the
 * need to add more `Box`es.
 *
 * Why `CollectionShape`s and `MapShape`s? Note that `CollectionShape`s get rendered in Rust as `Vec<T>`, and
 * `MapShape`s as `HashMap<String, T>`; they're the only Smithy shapes that "organically" introduce indirection
 * (via a pointer to the heap) in the recursive path. For other recursive paths, we thus have to introduce the
 * indirection artificially ourselves using `Box`.
 *
 */
private fun containsIndirection(loop: Collection<Shape>): Boolean = loop.find {
    when (it) {
        is CollectionShape, is MapShape -> true
        else -> it.hasTrait<RustBoxTrait>()
    }
} != null

private fun addRustBoxTrait(shape: MemberShape): MemberShape = shape.toBuilder().addTrait(RustBoxTrait()).build()
