/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.ec2

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.ClientOptionalTrait
import software.amazon.smithy.model.transform.ModelTransformer

object EC2MakePrimitivesOptional {
    fun processModel(model: Model): Model {
        val updates = arrayListOf<Shape>()
        for (struct in model.structureShapes) {
            for (member in struct.allMembers.values) {
                updates.add(member.toBuilder().addTrait(ClientOptionalTrait()).build())
            }
        }
        return ModelTransformer.create().replaceShapes(model, updates)
    }
}
