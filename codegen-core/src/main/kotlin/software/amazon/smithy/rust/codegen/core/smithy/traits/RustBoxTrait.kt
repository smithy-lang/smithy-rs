/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.traits

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.Trait

/**
 * Trait indicating that this shape should be represented with `Box<T>` when converted into Rust
 *
 * This is used to handle recursive shapes.
 * See [software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer].
 *
 * This trait is synthetic, applied during code generation, and never used in actual models.
 */
class RustBoxTrait : Trait {
    val ID = ShapeId.from("software.amazon.smithy.rust.codegen.smithy.rust.synthetic#box")
    override fun toNode(): Node = Node.objectNode()

    override fun toShapeId(): ShapeId = ID
}
