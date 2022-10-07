/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.hasConstraintTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape

/**
 * TODO Docs
 * TODO Move to server
 */
object AttachValidationExceptionToConstrainedOperationInputsInAllowList {
    // TODO Use fully qualified shapeIds
    private val sherviceShapeIdAllowList =
        setOf(
            ShapeId.from("aws.protocoltests.restjson#RestJson"),
//            ShapeId.from("abacacacacacacac"),
//            ShapeId.from("ababababababaaaaaaaaaaaaaa"),
        )

    fun transform(model: Model): Model {
//        model.serviceShapes.filter { it.toShapeId().name == "RestJsonValidation" }[0].operations.map { model.expectShape(it, OperationShape::class.java) }.map { it.errors.add(
//            ShapeId.from("smithy.framework#ValidationException")) }

        val walker = Walker(model)

        val operationsWithConstrainedInputWithoutValidationException = model.serviceShapes
            .filter { sherviceShapeIdAllowList.contains(it.toShapeId()) }
            .flatMap { it.operations }
            .map { model.expectShape(it, OperationShape::class.java) }
            .filter { operationShape ->
                // Walk the shapes reachable via this operation input.
                walker.walkShapes(operationShape.inputShape(model))
                    .any { it is SetShape || it is EnumShape || it.hasConstraintTrait() }
            }
            .filter { !it.errors.contains(ShapeId.from("smithy.framework#ValidationException")) }
//            .map { it.errors }
//            .forEach {
//                it.errors.add(ShapeId.from("smithy.framework#ValidationException"))
//            }
//        val inputShapes = model.operationShapes.map {
//            model.expectShape(it.inputShape, StructureShape::class.java)
//        }
//        val walker = Walker(model)
//        val shapesReachableFromOperationInputs = inputShapes
//            .flatMap { walker.walkShapes(it) }
//            .toSet()

        return ModelTransformer.create().mapShapes(model) { shape ->
            if (shape is OperationShape && operationsWithConstrainedInputWithoutValidationException.contains(shape)) {
                shape.toBuilder().addError("smithy.framework#ValidationException").build()
            } else {
                shape
            }
        }
//        for (errors in errorsInOperationsWithConstrainedInput) {
//            errors.add(ShapeId.from("smithy.framework#ValidationException"))
//        }
//        return model
    }
}
