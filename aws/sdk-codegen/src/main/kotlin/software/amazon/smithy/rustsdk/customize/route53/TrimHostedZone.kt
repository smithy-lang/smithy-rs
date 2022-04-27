/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk.customize.route53

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AnnotationTrait

/**
 * Indicates that a member should have the `hostedzone` prefix stripped
 */
class TrimHostedZone() : AnnotationTrait(ID, Node.objectNode()) {
    companion object {
        val ID = ShapeId.from("aws.api.internal#trimHostedZone")
    }
}
