/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import software.amazon.smithy.aws.traits.protocols.Ec2QueryNameTrait
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.XmlNameTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.utils.StringUtils

class Ec2QuerySerializerGenerator(codegenContext: CodegenContext) : QuerySerializerGenerator(codegenContext) {
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

    override fun RustWriter.serializeCollection(
        memberContext: MemberContext,
        context: Context<CollectionShape>,
    ) {
        rustBlock("if !${context.valueExpression.asRef()}.is_empty()") {
            super.serializeCollectionInner(memberContext, context, this)
        }
    }
}
