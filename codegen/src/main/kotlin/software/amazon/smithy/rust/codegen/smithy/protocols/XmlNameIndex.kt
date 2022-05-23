/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.KnowledgeIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.XmlAttributeTrait
import software.amazon.smithy.model.traits.XmlNameTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape

/**
 * KnowledgeIndex to determine the name for a given shape based on the XmlName trait and the shape's id.
 */
class XmlNameIndex(private val model: Model) : KnowledgeIndex {
    companion object {
        fun of(model: Model): XmlNameIndex {
            return model.getKnowledge(XmlNameIndex::class.java, ::XmlNameIndex)
        }
    }

    fun payloadShapeName(member: MemberShape): String {
        val payloadShape = model.expectShape(member.target)
        val xmlRename: XmlNameTrait? = member.getTrait() ?: payloadShape.getTrait()
        return xmlRename?.value ?: payloadShape.id.name
    }

    /**
     * XmlName for an operation output
     *
     * When an operation has no output body, null is returned
     */
    fun operationOutputShapeName(operationShape: OperationShape): String? {
        val outputShape = operationShape.outputShape(model)
        val rename = outputShape.getTrait<XmlNameTrait>()?.value
        return rename ?: outputShape.expectTrait<SyntheticOutputTrait>().originalId?.name
    }

    fun operationInputShapeName(operationShape: OperationShape): String? {
        val inputShape = operationShape.inputShape(model)
        val rename = inputShape.getTrait<XmlNameTrait>()?.value
        return rename ?: inputShape.expectTrait<SyntheticInputTrait>().originalId?.name
    }

    fun memberName(member: MemberShape): String {
        val override = member.getTrait<XmlNameTrait>()?.value
        return override ?: member.memberName
    }
}

data class XmlMemberIndex(val dataMembers: List<MemberShape>, val attributeMembers: List<MemberShape>) {
    companion object {
        fun fromMembers(members: List<MemberShape>): XmlMemberIndex {
            val (attribute, data) = members.partition { it.hasTrait<XmlAttributeTrait>() }
            return XmlMemberIndex(data, attribute)
        }
    }

    fun isEmpty() = dataMembers.isEmpty() && attributeMembers.isEmpty()
    fun isNotEmpty() = !isEmpty()
}
