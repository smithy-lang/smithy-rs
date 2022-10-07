/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.traits

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.Trait

/**
 * Trait indicating that this shape should be serialized, even when equal to 0 (or 0.0 for floats)
 *
 * This is used to mitigate issues that arise from APIs expecting 0 values
 * because operation serialization normally skips serializing 0 values.
 */
class SerializeZeroValues : Trait {
    val ID = ShapeId.from("smithy.api.internal#serializeZeroValues")
    override fun toNode(): Node = Node.objectNode()

    override fun toShapeId(): ShapeId = ID
}
