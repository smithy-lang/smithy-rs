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
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.util.orNull
import java.util.Optional
import kotlin.streams.toList

typealias StructureModifier = (OperationShape, StructureShape?) -> StructureShape?

/**
 * Generate synthetic Input and Output structures for operations.
 */
class OperationNormalizer(private val model: Model) {
    /**
     * Add synthetic input & output shapes to every Operation in model. The generated shapes will be marked with
     * [SyntheticInputTrait] and [SyntheticOutputTrait] respectively. Shapes will be added _even_ if the operation does
     * not specify an input or an output.
     */
    fun transformModel(): Model {
        val transformer = ModelTransformer.create()
        val operations = model.shapes(OperationShape::class.java).toList()
        val newShapes = operations.flatMap { operation ->
            // Generate or modify the input and output of the given `Operation` to be a unique shape
            syntheticInputShapes(operation) + syntheticOutputShapes(operation)
        }
        val modelWithOperationInputs = model.toBuilder().addShapes(newShapes).build()
        return transformer.mapShapes(modelWithOperationInputs) {
            // Update all operations to point to their new input/output shapes
            val transformed: Optional<Shape> = it.asOperationShape().map { operation ->
                modelWithOperationInputs.expectShape(operation.syntheticInputId())
                operation.toBuilder()
                    .input(operation.syntheticInputId())
                    .output(operation.syntheticOutputId())
                    .build()
            }
            transformed.orElse(it)
        }
    }

    private fun syntheticOutputShapes(operation: OperationShape): List<StructureShape> {
        val outputId = operation.syntheticOutputId()
        val outputShapeBuilder = operation.output.map { shapeId ->
            model.expectShape(shapeId, StructureShape::class.java).toBuilder().rename(outputId)
        }.orElse(empty(outputId))
        val outputShape = outputShapeBuilder.addTrait(
            SyntheticOutputTrait(
                operation = operation.id,
                originalId = operation.output.orNull(),
            )
        ).build()
        return listOfNotNull(outputShape)
    }

    private fun syntheticInputShapes(operation: OperationShape): List<StructureShape> {
        val inputId = operation.syntheticInputId()
        val inputShapeBuilder = operation.input.map { shapeId ->
            model.expectShape(shapeId, StructureShape::class.java).toBuilder().rename(inputId)
        }.orElse(empty(inputId))
        val inputShape =
            inputShapeBuilder.addTrait(
                SyntheticInputTrait(
                    operation = operation.id,
                    originalId = operation.input.orNull()
                )
            ).build()
        return listOfNotNull(inputShape)
    }

    private fun empty(id: ShapeId) = StructureShape.builder().id(id)

    companion object {
        // Functions to construct synthetic shape IDs—Don't rely on these in external code.
        // Rename safety: Operations cannot be renamed
        private fun OperationShape.syntheticInputId() = ShapeId.fromParts(this.id.namespace, "${this.id.name}Input")
        private fun OperationShape.syntheticOutputId() = ShapeId.fromParts(this.id.namespace, "${this.id.name}Output")
    }
}

private fun StructureShape.Builder.rename(newId: ShapeId): StructureShape.Builder {
    val renamedMembers = this.build().members().map {
        it.toBuilder().id(newId.withMember(it.memberName)).build()
    }
    return this.id(newId).members(renamedMembers)
}
