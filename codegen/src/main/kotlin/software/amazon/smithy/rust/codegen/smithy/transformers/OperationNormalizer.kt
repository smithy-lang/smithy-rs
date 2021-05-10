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

typealias StructureModifier = (OperationShape, StructureShape?) -> StructureShape?

/**
 * Generate synthetic Input and Output structures for operations.
 */
class OperationNormalizer(private val model: Model) {
    /**
     * Add synthetic input & output shapes to every Operation in model. The generated shapes will be marked with
     * [SyntheticInputTrait] and [SyntheticOutputTrait] respectively. Shapes will be added _even_ if the operation does
     * not specify an input or an output. The body shapes (if created) will be marked with [InputBodyTrait] and
     * [OutputBodyTrait]
     *
     * To build input bodies, [inputBodyFactory] is called on the input shape of every operation. It MUST return a structure representing the
     * shape of the request body, or null if there is no request body.
     *
     * To build output bodies, [outputBodyFactory] is called on the output shape of every operation. It MUST return a structure representing the
     * shape of the response body, or null if there is no response body.
     */
    fun transformModel(inputBodyFactory: StructureModifier, outputBodyFactory: StructureModifier): Model {
        val transformer = ModelTransformer.create()
        val operations = model.shapes(OperationShape::class.java).toList()
        val newShapes = operations.flatMap { operation ->
            // Generate or modify the input of input `Operation` to be a unique shape
            syntheticInputShapes(operation, inputBodyFactory) + syntheticOutputShapes(operation, outputBodyFactory)
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

    private fun syntheticOutputShapes(
        operation: OperationShape,
        outputBodyFactory: StructureModifier
    ): List<StructureShape> {
        val outputId = operation.outputId()
        val outputBodyShape = outputBodyFactory(
            operation,
            operation.output.map { model.expectShape(it, StructureShape::class.java) }.orNull()
        )?.let { it.toBuilder().addTrait(OutputBodyTrait()).rename(operation.outputBodyId()).build() }
        val outputShapeBuilder = operation.output.map { shapeId ->
            model.expectShape(shapeId, StructureShape::class.java).toBuilder().rename(outputId)
        }.orElse(empty(outputId))
        val outputShape = outputShapeBuilder.addTrait(
            SyntheticOutputTrait(
                operation = operation.id,
                originalId = operation.output.orNull(),
                body = outputBodyShape?.id
            )
        ).build()
        return listOf(outputShape, outputBodyShape).mapNotNull { it }
    }

    private fun syntheticInputShapes(
        operation: OperationShape,
        inputBodyFactory: StructureModifier
    ): List<StructureShape> {
        val inputId = operation.inputId()
        val inputBodyShape = inputBodyFactory(
            operation,
            operation.input.map {
                val inputShape = model.expectShape(it, StructureShape::class.java)
                inputShape.toBuilder().addTrait(InputBodyTrait()).rename(operation.inputBodyId()).build()
            }.orNull()
        )
        val inputShapeBuilder = operation.input.map { shapeId ->
            model.expectShape(shapeId, StructureShape::class.java).toBuilder().rename(inputId)
        }.orElse(empty(inputId))
        val inputShape =
            inputShapeBuilder.addTrait(
                SyntheticInputTrait(
                    operation = operation.id,
                    body = inputBodyShape?.id,
                    originalId = operation.input.orNull()
                )
            ).build()
        return listOf(inputShape, inputBodyShape).mapNotNull { it }
    }

    private fun empty(id: ShapeId) = StructureShape.builder().id(id)

    companion object {
        // Functions to construct synthetic shape IDs—Don't rely on these in external code: The attached traits
        // provide shape ids via `.body` on [SyntheticInputTrait] and [SyntheticOutputTrait]
        // Rename safety: Operations cannot be renamed
        private fun OperationShape.inputId() = ShapeId.fromParts(this.id.namespace, "${this.id.name}Input")
        private fun OperationShape.outputId() = ShapeId.fromParts(this.id.namespace, "${this.id.name}Output")
        private fun OperationShape.inputBodyId() = ShapeId.fromParts(this.id.namespace, "${this.id.name}InputBody")
        private fun OperationShape.outputBodyId() = ShapeId.fromParts(this.id.namespace, "${this.id.name}OutputBody")

        val NoBody: StructureModifier = { _: OperationShape, _: StructureShape? -> null }
    }
}

private fun StructureShape.Builder.rename(newId: ShapeId): StructureShape.Builder {
    val renamedMembers = this.build().members().map {
        it.toBuilder().id(newId.withMember(it.memberName)).build()
    }
    return this.id(newId).members(renamedMembers)
}
