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
import software.amazon.smithy.rust.codegen.smithy.traits.OutputBodyTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.util.orNull
import java.util.Optional
import kotlin.streams.toList

typealias StructureModifier = (StructureShape?) -> StructureShape?

/**
 * Generate synthetic Input and Output structures for operations.
 */
// TODO: generate operation outputs as well; 2h

class OperationNormalizer() {
    /**
     * Add synthetic input shapes to every Operation in model
     */
    fun transformModel(model: Model, inputBody: StructureModifier, outputBody: StructureModifier): Model {
        val transformer = ModelTransformer.create()
        val operations = model.shapes(OperationShape::class.java).toList()
        val newShapes = operations.flatMap { operation ->
            // Generate or modify the input of input `Operation` to be a unique shape
            syntheticInputShapes(operation, inputBody, model) + syntheticOutputShapes(operation, outputBody, model)
        }
        val modelWithOperationInputs = model.toBuilder().addShapes(newShapes).build()
        return transformer.mapShapes(modelWithOperationInputs) {
            // Update all operations to point to their new input shape
            val transformed: Optional<Shape> = it.asOperationShape().map { operation ->
                modelWithOperationInputs.expectShape(operation.inputId())
                operation.toBuilder()
                    .input(operation.inputId())
                    .output(operation.outputId())
                    .build()
            }
            transformed.orElse(it)
        }
    }

    private fun syntheticOutputShapes(operation: OperationShape, outputBody: StructureModifier, model: Model): List<StructureShape> {
        val outputId = operation.outputId()
        val outputBodyShape = outputBody(
            operation.output.map { model.expectShape(it, StructureShape::class.java) }.orNull()
        )?.let { it.toBuilder().addTrait(OutputBodyTrait()).rename(operation.outputBodyId()).build() }
        val outputShapeBuilder = operation.output.map { shapeId ->
            model.expectShape(shapeId, StructureShape::class.java).toBuilder().rename(outputId)
        }.orElse(empty(outputId))
        val outputShape = outputShapeBuilder.addTrait(SyntheticOutputTrait(outputBodyShape?.id)).build()
        return listOf(outputShape, outputBodyShape).mapNotNull { it }
    }

    private fun syntheticInputShapes(
        operation: OperationShape,
        inputBody: StructureModifier,
        model: Model
    ): List<StructureShape> {
        val inputId = operation.inputId()
        val inputBodyShape = inputBody(
            operation.input.map { model.expectShape(it, StructureShape::class.java) }.orNull()
        )?.let { it.toBuilder().addTrait(InputBodyTrait()).rename(operation.inputBodyId()).build() }
        val inputShapeBuilder = operation.input.map { shapeId ->
            model.expectShape(shapeId, StructureShape::class.java).toBuilder().rename(inputId)
        }.orElse(empty(inputId))
        val inputShape = inputShapeBuilder.addTrait(SyntheticInputTrait(inputBodyShape?.id)).build()
        return listOf(inputShape, inputBodyShape).mapNotNull { it }
    }

    private fun empty(id: ShapeId) = StructureShape.builder().id(id)

    companion object {
        private fun OperationShape.inputId() = ShapeId.fromParts(this.id.namespace, "${this.id.name}Input")
        private fun OperationShape.outputId() = ShapeId.fromParts(this.id.namespace, "${this.id.name}Output")
        private fun OperationShape.inputBodyId() = ShapeId.fromParts(this.id.namespace, "${this.id.name}InputBody")
        private fun OperationShape.outputBodyId() = ShapeId.fromParts(this.id.namespace, "${this.id.name}OutputBody")

        val noBody: (StructureShape?) -> StructureShape? = { _ -> null }
    }
}

private fun StructureShape.Builder.rename(newId: ShapeId): StructureShape.Builder {
    val renamedMembers = this.build().members().map {
        it.toBuilder().id(newId.withMember(it.memberName)).build()
    }
    return this.id(newId).members(renamedMembers)
}
