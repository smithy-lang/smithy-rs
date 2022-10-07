/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.util.orNull

/**
 * TODO Docs
 * TODO Tag issue
 * TODO Move to server
 */
object RemoveEbsModelValidationException {
    fun transform(model: Model): Model {
        val shapeToRemove = model.getShape(ShapeId.from("com.amazonaws.ebs#ValidationException")).orNull()
        return ModelTransformer.create().removeShapes(model, listOfNotNull(shapeToRemove))
    }
}
