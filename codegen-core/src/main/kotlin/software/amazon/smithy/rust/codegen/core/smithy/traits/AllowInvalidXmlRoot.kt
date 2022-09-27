/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.traits

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AnnotationTrait

/**
 * Indicates that a service is expected to send XML where the root element name does not match the modeled member name.
 */
class AllowInvalidXmlRoot : AnnotationTrait(ID, Node.objectNode()) {
    companion object {
        val ID: ShapeId = ShapeId.from("smithy.api.internal#allowInvalidXmlRoot")
    }
}
