/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.InputTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.rename
import java.util.Optional
import kotlin.streams.toList

/**
 * Generate synthetic Input and Output structures for operations.
 *
 * This works by **adding** new shapes to the model for operation inputs & outputs. These new shapes have `SyntheticInputTrait`
 * and `SyntheticOutputTrait` attached to them as needed. This enables downstream code generators to determine if a shape is
 * "real" vs. a shape created as a synthetic input/output.
 *
 * The trait also tracks the original shape id for certain serialization tasks that require it to exist.
 *
 * Note that adding/removing operation input/output [is a breaking change]; the only reason why we synthetically add them
 * is to produce a consistent API.
 *
 * [is a breaking change]: <https://github.com/smithy-lang/smithy/issues/2253#issuecomment-2069943344>
 */
object OperationNormalizer {
    // Functions to construct synthetic shape IDs—Don't rely on these in external code.
    // Rename safety: Operations cannot be renamed
    // In order to ensure that the fully qualified smithy id of a synthetic shape can never conflict with an existing shape,
    // the `synthetic` namespace is appended.
    private fun OperationShape.syntheticInputId() =
        ShapeId.fromParts(this.id.namespace + ".synthetic", "${this.id.name}Input")

    private fun OperationShape.syntheticOutputId() =
        ShapeId.fromParts(this.id.namespace + ".synthetic", "${this.id.name}Output")

    /**
     * Returns `true` if the user had originally modeled an operation input shape on the given [operation];
     * `false` if the transform added a synthetic one.
     */
    fun hadUserModeledOperationInput(
        operation: OperationShape,
        model: Model,
    ): Boolean {
        val syntheticInputTrait = operation.inputShape(model).expectTrait<SyntheticInputTrait>()
        return syntheticInputTrait.originalId != null
    }

    /**
     * Returns `true` if the user had originally modeled an operation output shape on the given [operation];
     * `false` if the transform added a synthetic one.
     */
    fun hadUserModeledOperationOutput(
        operation: OperationShape,
        model: Model,
    ): Boolean {
        val syntheticOutputTrait = operation.outputShape(model).expectTrait<SyntheticOutputTrait>()
        return syntheticOutputTrait.originalId != null
    }

    /**
     * Add synthetic input & output shapes to every Operation in model. The generated shapes will be marked with
     * [SyntheticInputTrait] and [SyntheticOutputTrait] respectively. Shapes will be added _even_ if the operation does
     * not specify an input or an output.
     *
     * The shapes are generated and added to the model, and the operation shapes are rebuilt to have their input and output ids
     * set accordingly.
     */
    fun transform(model: Model): Model {
        val transformer = ModelTransformer.create()
        val operations = model.shapes(OperationShape::class.java).toList()
        val newShapes =
            operations.flatMap { operation ->
                // Generate or modify the input and output of the given `Operation` to be a unique shape
                listOf(syntheticInputShape(model, operation), syntheticOutputShape(model, operation))
            }
        val shapeConflict = newShapes.firstOrNull { shape -> model.getShape(shape.id).isPresent }
        check(
            shapeConflict == null,
        ) {
            "shape $shapeConflict conflicted with an existing shape in the model (${model.expectShape(shapeConflict!!.id)}). This is a bug."
        }
        val modelWithOperationInputs = model.toBuilder().addShapes(newShapes).build()
        return transformer.mapShapes(modelWithOperationInputs) {
            // Update all operations to point to their new input/output shapes
            val transformed: Optional<Shape> =
                it.asOperationShape().map { operation ->
                    modelWithOperationInputs.expectShape(operation.syntheticInputId())
                    operation.toBuilder()
                        .input(operation.syntheticInputId())
                        .output(operation.syntheticOutputId())
                        .build()
                }
            transformed.orElse(it)
        }
    }

    /**
     * Synthetic output shape for `operation`
     * e.g. `<SomeOperation>Output`
     *
     * If the operation does not have an output, an empty shape is generated
     */
    private fun syntheticOutputShape(
        model: Model,
        operation: OperationShape,
    ): StructureShape {
        val outputId = operation.syntheticOutputId()
        val outputShapeBuilder =
            operation.output.map { shapeId ->
                model.expectShape(shapeId, StructureShape::class.java).toBuilder().rename(outputId)
            }.orElse(empty(outputId))
        return outputShapeBuilder.addTrait(
            SyntheticOutputTrait(
                operation = operation.id,
                originalId = operation.output.orNull(),
            ),
        ).build()
    }

    /**
     * Synthetic input shape for `operation`
     * e.g. `<SomeOperation>Input`
     *
     * If the input operation does not have an input, an empty shape is generated
     */
    private fun syntheticInputShape(
        model: Model,
        operation: OperationShape,
    ): StructureShape {
        val inputId = operation.syntheticInputId()
        val inputShapeBuilder =
            operation.input.map { shapeId ->
                model.expectShape(shapeId, StructureShape::class.java).toBuilder().rename(inputId)
            }.orElse(empty(inputId))
        // There are still shapes missing the input trait. If we don't apply this, we'll get bad results from the
        // nullability index
        if (!inputShapeBuilder.build().hasTrait<InputTrait>()) {
            inputShapeBuilder.addTrait(InputTrait())
        }
        return inputShapeBuilder.addTrait(
            SyntheticInputTrait(
                operation = operation.id,
                originalId = operation.input.orNull(),
            ),
        ).build()
    }

    private fun empty(id: ShapeId) = StructureShape.builder().id(id)
}
