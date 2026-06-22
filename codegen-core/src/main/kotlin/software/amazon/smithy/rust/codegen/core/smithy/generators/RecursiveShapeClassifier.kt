/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId

/**
 * Classifies whether emitting a nested aggregate's resolved schema constant
 * from its containing aggregate would close a cycle in the `static` schema
 * data graph.
 *
 * Used by [SchemaGenerator] to decide whether a nested list/map element can
 * reference the resolved sub-schema constant (e.g. `&FOO_MEMBER`) — preserving
 * the element's member traits such as `@xmlName` / `@xmlFlattened` — or must
 * fall back to `&prelude::DOCUMENT`.
 *
 * Reachability is computed over **aggregate edges only**: list elements and map
 * key/value targets. Structure and union targets terminate the walk because
 * each carries its own top-level `::SCHEMA` constant, so a path that reaches the
 * containing aggregate only by passing through a structure or union does **not**
 * close a cycle in the inline `static` schema data — it crosses a named-constant
 * boundary that breaks the chain. Only an aggregate-only cycle (a list/map that
 * reaches itself without an intervening structure/union) would close such a
 * cycle, and Smithy forbids those, so for any valid model the resolved constant
 * is always finite and safe to reference.
 *
 * This matches the data emitted by [SchemaGenerator.emitAggregateMemberChain],
 * which likewise descends only through aggregates and stops at struct/union.
 *
 * Closures are computed lazily and cached per target shape, so callers can ask
 * many questions across one codegen run without re-walking the shape graph for
 * the same target.
 */
class RecursiveShapeClassifier(private val model: Model) {
    private val aggregateClosures = mutableMapOf<ShapeId, Set<ShapeId>>()

    /**
     * Returns true if [target] can reach [containingAggregate] by following only
     * aggregate edges (list element, map key/value), treating structure and
     * union shapes as opaque leaves.
     *
     * Intended to be called when [containingAggregate] has a direct
     * member-target edge to [target] (i.e. when the codegen is about to render
     * that very edge as a schema reference). Under that condition, a `true`
     * result means emitting [target]'s schema as a direct reference from
     * [containingAggregate]'s schema would close a cycle in the `static` data
     * graph; the caller should fall back to `prelude::DOCUMENT` instead.
     *
     * A self-referential target (`target == containingAggregate`) counts as
     * recursive, because the closure includes the start shape.
     */
    fun isRecursive(
        containingAggregate: Shape,
        target: Shape,
    ): Boolean {
        val closure = aggregateClosures.getOrPut(target.id) { aggregateClosure(target) }
        return containingAggregate.id in closure
    }

    /**
     * The set of shapes reachable from [start] by following only aggregate edges
     * (list element, map key/value). Structure and union shapes are added to the
     * set but the walk does not descend through them (they own a separate
     * `::SCHEMA` constant). The `seen` set also guarantees termination even for a
     * malformed model that contained an aggregate-only cycle.
     */
    private fun aggregateClosure(start: Shape): Set<ShapeId> {
        val seen = mutableSetOf<ShapeId>()
        val stack = ArrayDeque<Shape>()
        stack.addLast(start)
        while (stack.isNotEmpty()) {
            val shape = stack.removeLast()
            if (!seen.add(shape.id)) continue
            val next =
                when (shape) {
                    // `ListShape` also covers the deprecated `SetShape` (a subclass).
                    is ListShape -> listOf(shape.member.target)
                    is MapShape -> listOf(shape.key.target, shape.value.target)
                    // Structure / union (own `::SCHEMA` constant) and scalars terminate the walk.
                    else -> emptyList()
                }
            next.forEach { stack.addLast(model.expectShape(it)) }
        }
        return seen
    }
}
