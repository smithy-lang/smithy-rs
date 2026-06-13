/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker

/**
 * Classifies whether a member's target shape transitively references the
 * containing aggregate.
 *
 * Used by [SchemaGenerator] to decide whether to emit the actual nested
 * aggregate's schema constant (e.g. `&FOO_VALUE_SCHEMA`) or to fall back
 * to `&prelude::DOCUMENT` for self-referential types. Recursive aggregates
 * (e.g. DynamoDB's `AttributeValue` map values) must keep emitting
 * `prelude::DOCUMENT` because the runtime walks the same `SCHEMA` constant
 * on each level — emitting a direct reference would create a cycle in the
 * `static` data the codegen lays down.
 *
 * Closures are computed lazily and cached per target shape, so callers can
 * ask many questions across one codegen run without re-walking the shape
 * graph for the same target.
 */
class RecursiveShapeClassifier(model: Model) {
    private val walker = DirectedWalker(model)
    private val targetClosures = mutableMapOf<ShapeId, Set<ShapeId>>()

    /**
     * Returns true if [target] can reach [containingAggregate] via a chain
     * of directed shape-graph edges.
     *
     * Intended to be called when [containingAggregate] has a direct
     * member-target edge to [target] (i.e. when the codegen is about to
     * render that very edge as a schema reference). Under that condition,
     * a `true` result means emitting [target]'s schema as a direct
     * reference from [containingAggregate]'s schema would close a cycle
     * in the `static` data graph; the caller should fall back to
     * `prelude::DOCUMENT` instead.
     *
     * Asking in the reverse direction (no model edge from
     * [containingAggregate] to [target]) is well-defined but not what
     * this classifier is for: the answer reflects pure reachability and
     * does not predict cycle creation.
     *
     * A self-referential target (`target == containingAggregate`) counts
     * as recursive, because [DirectedWalker.walkShapes] includes the
     * start shape in its result.
     */
    fun isRecursive(
        containingAggregate: Shape,
        target: Shape,
    ): Boolean {
        val closure =
            targetClosures.getOrPut(target.id) {
                walker.walkShapes(target).map { it.id }.toSet()
            }
        return containingAggregate.id in closure
    }
}
