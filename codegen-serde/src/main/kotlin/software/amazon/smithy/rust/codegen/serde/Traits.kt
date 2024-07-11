/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AbstractTrait
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.core.util.orNull

class SerdeTrait private constructor(
    val serialize: Boolean,
    val deserialize: Boolean,
    val tag: String?,
    val content: String?,
    sourceLocation: SourceLocation,
) :
    AbstractTrait(ID, sourceLocation) {
        override fun createNode(): Node {
            val builder =
                Node.objectNodeBuilder()
                    .sourceLocation(sourceLocation)
                    .withMember("serialize", serialize)
                    .withMember("deserialize", deserialize)
                    .withMember("tag", tag)
                    .withMember("content", content)
            return builder.build()
        }

        class Provider : AbstractTrait.Provider(ID) {
            override fun createTrait(
                target: ShapeId,
                value: Node,
            ): Trait {
                val serialize = value.expectObjectNode().getBooleanMember("serialize").map { it.value }.orElse(true)
                val deserialize = value.expectObjectNode().getBooleanMember("deserialize").map { it.value }.orElse(true)
                val tag = value.expectObjectNode().getStringMember("tag").map { it.value }.orNull()
                val content = value.expectObjectNode().getStringMember("content").map { it.value }.orNull()
                val result =
                    SerdeTrait(
                        serialize,
                        deserialize,
                        tag,
                        content,
                        value.sourceLocation,
                    )
                result.setNodeCache(value)
                return result
            }
        }

        companion object {
            val ID = ShapeId.from("smithy.rust#serde")!!
        }
    }
