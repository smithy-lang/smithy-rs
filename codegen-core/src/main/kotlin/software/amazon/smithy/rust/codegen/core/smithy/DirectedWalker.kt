/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Relationship
import software.amazon.smithy.model.neighbor.RelationshipDirection
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.Shape
import java.util.function.Predicate

/**
 * A walker which traverses down the Shape graph. This is in contrast to `Walker` which can traverse up the shape
 * graph.
 */
class DirectedWalker(model: Model) {
    private val inner = Walker(model)

    fun walkShapes(shape: Shape): Set<Shape> = walkShapes(shape) { true }

    fun walkShapes(shape: Shape, predicate: Predicate<Relationship>): Set<Shape> =
        inner.walkShapes(shape) { rel -> predicate.test(rel) && rel.direction == RelationshipDirection.DIRECTED }
}
