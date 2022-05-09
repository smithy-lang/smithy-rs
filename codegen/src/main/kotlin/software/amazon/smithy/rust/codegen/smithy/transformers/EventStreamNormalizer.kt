/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticEventStreamUnionTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.isEventStream

/**
 * Generates synthetic unions to replace the modeled unions for Event Stream types.
 * This allows us to strip out all the error union members once up-front, instead of in each
 * place that does codegen with the unions.
 */
object EventStreamNormalizer {
    fun transform(model: Model): Model = ModelTransformer.create().mapShapes(model) { shape ->
        if (shape is UnionShape && shape.isEventStream()) {
            syntheticEquivalent(model, shape)
        } else {
            shape
        }
    }

    private fun syntheticEquivalent(model: Model, union: UnionShape): UnionShape {
        val (errorMembers, eventMembers) = union.members().partition { member ->
            model.expectShape(member.target).hasTrait<ErrorTrait>()
        }
        return union.toBuilder()
            .members(eventMembers)
            .addTrait(SyntheticEventStreamUnionTrait(errorMembers))
            .build()
    }
}
