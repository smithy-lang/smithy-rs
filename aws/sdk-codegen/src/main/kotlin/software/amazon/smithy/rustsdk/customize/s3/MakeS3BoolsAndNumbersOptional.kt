/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.AbstractShapeBuilder
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.DefaultTrait
import software.amazon.smithy.model.transform.ModelTransformer

/**
 * The s3 model is being updated but if we consume that model update, then we'll run into an issue with examples compilation
 *
 * This "pre-updates" the model so we can fix examples without requiring complex coordination
 */
class MakeS3BoolsAndNumbersOptional {
    fun processModel(model: Model): Model {
        val updates = arrayListOf<Shape>()
        for (struct in model.structureShapes) {
            for (member in struct.allMembers.values) {
                val target = model.expectShape(member.target)
                val boolTarget = target as? BooleanShape
                val numberTarget = target as? NumberShape
                if (boolTarget != null || numberTarget != null) {
                    updates.add(member.toBuilder().removeTrait(DefaultTrait.ID).build())
                    val builder: AbstractShapeBuilder<*, *> = Shape.shapeToBuilder(target)
                    updates.add(builder.removeTrait(DefaultTrait.ID).build())
                }
            }
        }
        return ModelTransformer.create().replaceShapes(model, updates)
    }
}
