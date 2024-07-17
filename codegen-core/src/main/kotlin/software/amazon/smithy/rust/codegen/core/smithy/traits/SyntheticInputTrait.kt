/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.traits

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AnnotationTrait

/**
 * Indicates that a shape is a synthetic input (see `OperationNormalizer.kt`)
 *
 * All operations are normalized to have an input, even when they are defined without one.
 * This is NOT done for backwards-compatibility, as adding an operation input is a breaking change
 * (see <https://github.com/smithy-lang/smithy/issues/2253#issuecomment-2069943344>).
 *
 * It is only done to produce a consistent API.
 * TODO(https://github.com/smithy-lang/smithy-rs/issues/3577): In the server, we'd like to stop adding
 *  these synthetic inputs.
 */
class SyntheticInputTrait(
    val operation: ShapeId,
    val originalId: ShapeId?,
) : AnnotationTrait(ID, Node.objectNode()) {
    companion object {
        val ID: ShapeId = ShapeId.from("smithy.api.internal#syntheticInput")
    }
}
