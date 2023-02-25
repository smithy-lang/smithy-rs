/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.traits

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.Trait

/**
 * This shape is analogous to [software.amazon.smithy.rust.codegen.core.smithy.traits.RustBoxTrait], but for the
 * constraint violation graph. The sets of shapes we tag are different, and they are interpreted by the code generator
 * differently, so we need a separate tag.
 *
 * This is used to handle recursive constraint violations.
 * See [software.amazon.smithy.rust.codegen.server.smithy.transformers.RecursiveConstraintViolationBoxer].
 */
class ConstraintViolationRustBoxTrait : Trait {
    val ID = ShapeId.from("software.amazon.smithy.rust.codegen.smithy.rust.synthetic#constraintViolationBox")
    override fun toNode(): Node = Node.objectNode()

    override fun toShapeId(): ShapeId = ID
}
