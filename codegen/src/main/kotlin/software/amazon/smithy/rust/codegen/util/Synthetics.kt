/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.util

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.ToShapeId

/**
 * Clones an entire operation and its input/output shapes under a new name.
 */
fun Model.Builder.cloneOperation(
    model: Model,
    oldOperation: ToShapeId,
    idTransform: (ShapeId) -> ShapeId
): Model.Builder {
    val operationShape = model.expectShape(oldOperation.toShapeId(), OperationShape::class.java)
    val inputShape = model.expectShape(
        checkNotNull(operationShape.input.orNull()) {
            "cloneOperation expects OperationNormalizer to be run first to add input shapes to all operations"
        },
        StructureShape::class.java
    )
    val outputShape = model.expectShape(
        checkNotNull(operationShape.output.orNull()) {
            "cloneOperation expects OperationNormalizer to be run first to add output shapes to all operations"
        },
        StructureShape::class.java
    )

    val inputId = idTransform(inputShape.id)
    addShape(inputShape.toBuilder().rename(inputId).build())
    val outputId = idTransform(outputShape.id)
    if (outputId != inputId) {
        addShape(outputShape.toBuilder().rename(outputId).build())
    }
    val operationId = idTransform(operationShape.id)
    addShape(
        operationShape.toBuilder()
            .id(operationId)
            .input(inputId)
            .output(outputId)
            .build()
    )
    return this
}

/**
 * Renames a StructureShape builder and automatically fixes all the members.
 */
fun StructureShape.Builder.rename(newId: ShapeId): StructureShape.Builder {
    val renamedMembers = this.build().members().map {
        it.toBuilder().id(newId.withMember(it.memberName)).build()
    }
    return this.id(newId).members(renamedMembers)
}
