/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId

/** Reject unsupported collection-only recursion before symbol providers can recurse into it. */
internal fun validateSchemaCollections(
    model: Model,
    service: ServiceShape,
) {
    val finished = mutableSetOf<ShapeId>()
    val active = linkedSetOf<ShapeId>()

    fun visit(shape: Shape) {
        val children =
            when (shape) {
                is ListShape -> listOf(shape.member)
                is MapShape -> listOf(shape.key, shape.value)
                else -> return // Structure and union boundaries have their own schema references.
            }
        if (shape.id in finished) return
        if (!active.add(shape.id)) {
            throw CodegenException("Schema serde does not yet support collection-only recursion: ${(active.toList() + shape.id).joinToString(" -> ")}")
        }
        children.forEach { visit(model.expectShape(it.target)) }
        active.remove(shape.id)
        finished.add(shape.id)
    }
    Walker(model).walkShapes(service).forEach(::visit)
}
