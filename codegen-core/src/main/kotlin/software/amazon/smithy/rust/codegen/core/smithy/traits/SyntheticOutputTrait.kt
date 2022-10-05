/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.traits

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AnnotationTrait

/**
 * Indicates that a shape is a synthetic output (see `OperationNormalizer.kt`)
 *
 * All operations are normalized to have an output, even when they are defined without on. This is done for backwards
 * compatibility and to produce a consistent API.
 */
class SyntheticOutputTrait constructor(val operation: ShapeId, val originalId: ShapeId?) :
    AnnotationTrait(ID, Node.objectNode()) {
    companion object {
        val ID: ShapeId = ShapeId.from("smithy.api.internal#syntheticOutput")
    }
}
