/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.traits

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AnnotationTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait

/**
 * Indicates that a shape must be set by users and that operations will fail to build if it's unset.
 */
class Mandatory : AnnotationTrait(ID, Node.objectNode()) {
    companion object {
        val ID: ShapeId = ShapeId.from("smithy.api.internal#mandatory")
    }
}

fun Shape.isMandatory() = this.hasTrait<Mandatory>()
