/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.traits

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AnnotationTrait

/**
 * Trait applied to an overridden shape indicating the member of this new shape type
 */
class SyntheticStructureFromConstrainedMemberTrait(val container: Shape, val member: MemberShape) : AnnotationTrait(SyntheticStructureFromConstrainedMemberTrait.ID, Node.objectNode()) {
    companion object {
        val ID: ShapeId = ShapeId.from("smithy.api.internal#overriddenMember")
    }
}
