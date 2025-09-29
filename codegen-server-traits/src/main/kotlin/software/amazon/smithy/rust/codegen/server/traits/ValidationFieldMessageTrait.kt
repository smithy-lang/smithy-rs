/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.traits

import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AbstractTrait
import software.amazon.smithy.model.traits.Trait

class ValidationFieldMessageTrait(sourceLocation: SourceLocation) : AbstractTrait(ID, sourceLocation) {
    companion object {
        val ID: ShapeId = ShapeId.from("smithy.rust.codegen.server.traits#validationFieldMessage")
    }

    override fun createNode(): Node = Node.objectNode()

    class Provider : AbstractTrait.Provider(ID) {
        override fun createTrait(
            target: ShapeId,
            value: Node,
        ): Trait {
            val result = ValidationFieldMessageTrait(value.sourceLocation)
            result.setNodeCache(value)
            return result
        }
    }
}
