/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticEventStreamUnionTrait
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.outputShape

/**
 * Generates synthetic unions to replace the modeled unions for Event Stream types.
 * This allows us to strip out all the error union members once up-front, instead of in each
 * place that does codegen with the unions.
 */
object EventStreamNormalizer {
    fun transform(model: Model): Model = ModelTransformer.create().mapShapes(model) { shape ->
        if (shape is OperationShape && shape.isEventStream(model)) {
            addStreamErrorsToOperationErrors(model, shape)
        } else if (shape is UnionShape && shape.isEventStream()) {
            syntheticEquivalentEventStreamUnion(model, shape)
        } else {
            shape
        }
    }

    private fun addStreamErrorsToOperationErrors(model: Model, operation: OperationShape): OperationShape {
        if (!operation.isEventStream(model)) {
            return operation
        }
        val getStreamErrors = { shape: ShapeId ->
            model.expectShape(shape).members()
                .filter { it.isEventStream(model) }
                .map { model.expectShape(it.target) }.flatMap { it.members() }
                .filter { model.expectShape(it.target).hasTrait<ErrorTrait>() }
                .map { model.expectShape(it.target).id }
        }
        val inputs = operation.input.map { getStreamErrors(it) }
        val outputs = operation.output.map { getStreamErrors(it) }
        val streamErrors = inputs.orElse(listOf()) + outputs.orElse(listOf())
        return operation.toBuilder()
            .addErrors(streamErrors.map { it.toShapeId() })
            .build()
    }

    private fun syntheticEquivalentEventStreamUnion(model: Model, union: UnionShape): UnionShape {
        val (errorMembers, eventMembers) = union.members().partition { member ->
            model.expectShape(member.target).hasTrait<ErrorTrait>()
        }
        return union.toBuilder()
            .members(eventMembers)
            .addTrait(SyntheticEventStreamUnionTrait(errorMembers))
            .build()
    }
}

fun OperationShape.operationErrors(model: Model): List<Shape> {
    val operationIndex = OperationIndex.of(model)
    return operationIndex.getErrors(this)
}

fun eventStreamErrors(model: Model, shape: Shape): Map<UnionShape, List<StructureShape>> {
    return DirectedWalker(model)
        .walkShapes(shape)
        .filter { it is UnionShape && it.isEventStream() }
        .map { it.asUnionShape().get() }
        .associateWith { unionShape ->
            unionShape.expectTrait<SyntheticEventStreamUnionTrait>().errorMembers
                .map { model.expectShape(it.target).asStructureShape().get() }
        }
}

fun UnionShape.eventStreamErrors(): List<Shape> {
    if (!this.isEventStream()) {
        return listOf()
    }
    return this.expectTrait<SyntheticEventStreamUnionTrait>().errorMembers
}

fun OperationShape.eventStreamErrors(model: Model): Map<UnionShape, List<StructureShape>> {
    return eventStreamErrors(model, inputShape(model)) + eventStreamErrors(model, outputShape(model))
}

fun OperationShape.allErrors(model: Model): List<Shape> {
    return this.eventStreamErrors(model).values.flatten() + this.operationErrors(model)
}
