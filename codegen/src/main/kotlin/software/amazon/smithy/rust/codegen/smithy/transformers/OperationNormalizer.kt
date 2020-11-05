/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.transformers

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInput
import java.util.Optional
import kotlin.streams.toList

/**
 * Generate synthetic Input and Output structures for operations.
 */
// TODO: generate operation outputs as well; 2h
class OperationNormalizer(private val symbolProvider: SymbolProvider) {
    fun addOperationInputs(model: Model): Model {
        val transformer = ModelTransformer.create()
        val newShapes = model.shapes(OperationShape::class.java).map { operation ->
            // Generate or modify the input of input `Operation` to be a unique shape
            val inputId = ShapeId.fromParts(operation.id.namespace, "${symbolProvider.toSymbol(operation).name}Input")
            val newInputShape = operation.input.map { shapeId ->
                val shape = model.expectShape(shapeId, StructureShape::class.java)
                val renamedMembers = shape.members().map {
                    it.toBuilder().id(inputId.withMember(it.memberName)).build()
                }
                shape.toBuilder().id(inputId).members(renamedMembers)
            }.orElse(StructureShape.builder().id(inputId))
            newInputShape.addTrait(SyntheticInput()).build()
        }.toList()
        val modelWithOperationInputs = model.toBuilder().addShapes(newShapes).build()
        return transformer.mapShapes(modelWithOperationInputs) {
            // Update all operations to point to their new input shape
            val transformed: Optional<Shape> = it.asOperationShape().map { operation ->
                val inputId = ShapeId.fromParts(operation.id.namespace, "${symbolProvider.toSymbol(operation).name}Input")
                modelWithOperationInputs.expectShape(inputId)
                operation.toBuilder().input(inputId).build()
            }
            transformed.orElse(it)
        }
    }
}
