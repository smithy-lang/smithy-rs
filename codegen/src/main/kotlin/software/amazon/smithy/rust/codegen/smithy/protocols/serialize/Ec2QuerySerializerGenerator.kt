/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.serialize

import software.amazon.smithy.aws.traits.protocols.Ec2QueryNameTrait
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.XmlNameTrait
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.utils.StringUtils

class Ec2QuerySerializerGenerator(coreCodegenContext: CoreCodegenContext) : QuerySerializerGenerator(coreCodegenContext) {
    override val protocolName: String get() = "EC2 Query"

    override fun MemberShape.queryKeyName(prioritizedFallback: String?): String =
        getTrait<Ec2QueryNameTrait>()?.value
            ?: getTrait<XmlNameTrait>()?.value?.let { StringUtils.capitalize(it) }
            ?: StringUtils.capitalize(memberName)

    override fun MemberShape.isFlattened(): Boolean = true

    override fun operationOutputSerializer(operationShape: OperationShape): RuntimeType? {
        TODO("Not yet implemented")
    }

    override fun serverErrorSerializer(shape: ShapeId): RuntimeType {
        TODO("Not yet implemented")
    }
}
