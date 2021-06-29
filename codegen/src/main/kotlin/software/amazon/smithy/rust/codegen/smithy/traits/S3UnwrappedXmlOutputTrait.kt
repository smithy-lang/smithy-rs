/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.traits

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AnnotationTrait

/**
 * S3's GetBucketLocation response shape can't be represented with Smithy's restXml protocol
 * without customization. We add this trait to the S3 model at codegen time so that a different
 * code path is taken in the XML deserialization codegen to generate code that parses the S3
 * response shape correctly.
 *
 * From what the S3 model states, the generated parser would expect:
 * ```
 * <LocationConstraint xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
 *     <LocationConstraint>us-west-2</LocationConstraint>
 * </LocationConstraint>
 * ```
 *
 * But S3 actually responds with:
 * ```
 * <LocationConstraint xmlns="http://s3.amazonaws.com/doc/2006-03-01/">us-west-2</LocationConstraint>
 * ```
 */
class S3UnwrappedXmlOutputTrait : AnnotationTrait(ID, Node.objectNode()) {
    companion object {
        val ID = ShapeId.from("smithy.api.internal#s3UnwrappedXmlOutputTrait")
    }
}
