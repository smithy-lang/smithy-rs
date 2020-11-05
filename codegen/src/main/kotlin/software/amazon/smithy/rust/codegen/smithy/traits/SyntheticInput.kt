/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.traits

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AnnotationTrait

/**
 * Indicates that a shape is a synthetic input (see `OperationNormalizer.kt`)
 */
class SyntheticInput @JvmOverloads constructor(node: ObjectNode? = Node.objectNode()) :
    AnnotationTrait(ID, node) {
    class Provider : AnnotationTrait.Provider<SyntheticInput?>(
        ID,
        { node: ObjectNode? ->
            SyntheticInput(
                node
            )
        }
    )

    companion object {
        val ID = ShapeId.from("smithy.api.internal#synthetic")
    }
}
