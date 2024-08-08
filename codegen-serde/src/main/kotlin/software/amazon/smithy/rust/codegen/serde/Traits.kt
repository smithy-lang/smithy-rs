/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import software.amazon.smithy.build.SmithyBuildException
import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AbstractTrait
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.core.util.orNull

class SerdeTrait constructor(
    private val serialize: Boolean,
    private val deserialize: Boolean,
    private val tag: String?,
    private val content: String?,
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
            ): Trait =
                with(value.expectObjectNode()) {
                    val serialize = getBooleanMemberOrDefault("serialize", true)
                    val deserialize = getBooleanMemberOrDefault("deserialize", false)
                    val tag = getStringMember("tag").orNull()?.value
                    val content = getStringMember("content").orNull()?.value
                    if (deserialize) {
                        throw SmithyBuildException("deserialize is not currently supported.")
                    }
                    val result =
                        SerdeTrait(
                            serialize,
                            deserialize,
                            tag,
                            content,
                            value.sourceLocation,
                        )
                    result.setNodeCache(value)
                    result
                }
        }

        companion object {
            val ID: ShapeId = ShapeId.from("smithy.rust#serde")
        }
    }
