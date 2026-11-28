/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.traits

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AnnotationTrait
import software.amazon.smithy.model.transform.ModelTransformer

/**
 * Marks a structure or union member that directly holds documents as using `DocumentV2` only.
 *
 * A marked member's Rust type holds `aws_smithy_types::DocumentV2` wherever an unmarked member's holds `Document`,
 * and it has no `Document` form: no companion, and no additive union variant. An unmarked member keeps the
 * side-by-side `Document` and `DocumentV2` API. See
 * [software.amazon.smithy.rust.codegen.core.smithy.generators.DocumentV2Index].
 *
 * This trait is synthetic, applied during code generation to the transformed model, and never used in actual models.
 * Only clients that use schema serde support it.
 */
class DocumentV2OnlyTrait : AnnotationTrait(ID, Node.objectNode()) {
    companion object {
        val ID: ShapeId = ShapeId.from("smithy.api.internal#documentV2Only")

        /**
         * Returns [model] with [DocumentV2OnlyTrait] on each of [members].
         *
         * [members] must be members of [model]. `DocumentV2Index` checks that each one is a structure or union member
         * that directly holds documents.
         */
        fun markMembers(
            model: Model,
            members: Collection<ShapeId>,
        ): Model {
            val marked =
                members.distinct().map { id ->
                    model.expectShape(id, MemberShape::class.java).toBuilder().addTrait(DocumentV2OnlyTrait()).build()
                }
            return ModelTransformer.create().replaceShapes(model, marked)
        }
    }
}
