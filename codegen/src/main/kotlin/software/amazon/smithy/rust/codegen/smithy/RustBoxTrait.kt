/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AnnotationTrait

/**
 * Trait indicating that this shape should be represented with `Box<T>` when converted into Rust
 *
 * This is used to handle recursive shapes. See RecursiveShapeBoxer.
 *
 * This trait is synthetic, applied during code generation, and never used in actual models.
 */
class RustBoxTrait : AnnotationTrait(ID, Node.objectNode()) {
    companion object {
        val ID: ShapeId = ShapeId.from("software.amazon.smithy.rust.codegen.smithy.rust.synthetic#box")
    }
}
