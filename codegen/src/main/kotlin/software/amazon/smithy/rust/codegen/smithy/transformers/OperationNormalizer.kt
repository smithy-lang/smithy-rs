/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.smithy.traits.InputBodyTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.util.orNull
import java.util.Optional
import kotlin.streams.toList

typealias structureModifier = (StructureShape?) -> StructureShape?

/**
 * Generate synthetic Input and Output structures for operations.
 */
// TODO: generate operation outputs as well; 2h

class OperationNormalizer() {
    /**
     * Add synthetic input shapes to every Operation in model
     */
    fun transformModel(model: Model, inputBody: structureModifier = noBody): Model {
        val transformer = ModelTransformer.create()
        val operations = model.shapes(OperationShape::class.java).toList()
        val newShapes = operations.flatMap { operation ->
            // Generate or modify the input of input `Operation` to be a unique shape
            val inputId = operation.inputId()
            val inputShapeBuilder = operation.input.map { shapeId ->
                val shape = model.expectShape(shapeId, StructureShape::class.java)
                val renamedMembers = shape.members().map {
                    it.toBuilder().id(inputId.withMember(it.memberName)).build()
                }
                shape.toBuilder().id(inputId).members(renamedMembers)
            }.orElse(StructureShape.builder().id(inputId))
            val bodyShape = inputBody(
                operation.input.map { model.expectShape(it, StructureShape::class.java) }.orNull()
            )?.let { it.toBuilder().addTrait(InputBodyTrait()).build().rename(operation.bodyId()) }
            val inputShape = inputShapeBuilder.addTrait(SyntheticInputTrait(bodyShape?.id)).build()
            listOf(bodyShape, inputShape).mapNotNull { it }
        }
        val modelWithOperationInputs = model.toBuilder().addShapes(newShapes).build()
        return transformer.mapShapes(modelWithOperationInputs) {
            // Update all operations to point to their new input shape
            val transformed: Optional<Shape> = it.asOperationShape().map { operation ->
                modelWithOperationInputs.expectShape(operation.inputId())
                operation.toBuilder().input(operation.inputId()).build()
            }
            transformed.orElse(it)
        }
    }

    companion object {
        private fun OperationShape.inputId() = ShapeId.fromParts(this.id.namespace, "${this.id.name}Input")
        private fun OperationShape.bodyId() = ShapeId.fromParts(this.id.namespace, "${this.id.name}InputBody")

        val noBody: (StructureShape?) -> StructureShape? = { _ -> null }
    }
}

private fun StructureShape.rename(newId: ShapeId): StructureShape {
    val renamedMembers = this.members().map {
        it.toBuilder().id(newId.withMember(it.memberName)).build()
    }
    return this.toBuilder().id(newId).members(renamedMembers).build()
}
