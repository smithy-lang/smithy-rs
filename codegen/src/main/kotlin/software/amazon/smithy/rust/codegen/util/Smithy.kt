/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.util

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.model.traits.Trait

inline fun <reified T : Shape> Model.lookup(shapeId: String): T {
    return this.expectShape(ShapeId.from(shapeId), T::class.java)
}

fun OperationShape.inputShape(model: Model): StructureShape {
    // The Rust Smithy generator adds an input to all shapes automatically
    return model.expectShape(this.input.get(), StructureShape::class.java)
}

fun OperationShape.outputShape(model: Model): StructureShape {
    // The Rust Smithy generator adds an output to all shapes automatically
    return model.expectShape(this.output.get(), StructureShape::class.java)
}

fun StructureShape.expectMember(member: String): MemberShape =
    this.getMember(member).orElseThrow { CodegenException("$member did not exist on $this") }

fun UnionShape.expectMember(member: String): MemberShape =
    this.getMember(member).orElseThrow { CodegenException("$member did not exist on $this") }

fun StructureShape.hasStreamingMember(model: Model) = this.findStreamingMember(model) != null
fun UnionShape.hasStreamingMember(model: Model) = this.findMemberWithTrait<StreamingTrait>(model) != null
fun MemberShape.isStreaming(model: Model) = this.getMemberTrait(model, StreamingTrait::class.java).isPresent

/*
 * Returns the member of this structure targeted with streaming trait (if it exists).
 *
 * A structure must have at most one streaming member.
 */
fun StructureShape.findStreamingMember(model: Model): MemberShape? {
    return this.findMemberWithTrait<StreamingTrait>(model)
}

inline fun <reified T : Trait> StructureShape.findMemberWithTrait(model: Model): MemberShape? {
    return this.members().find { it.getMemberTrait(model, T::class.java).isPresent }
}

inline fun <reified T : Trait> UnionShape.findMemberWithTrait(model: Model): MemberShape? {
    return this.members().find { it.getMemberTrait(model, T::class.java).isPresent }
}

/** Kotlin sugar for hasTrait() check. e.g. shape.hasTrait<EnumTrait>() instead of shape.hasTrait(EnumTrait::class.java) */
inline fun <reified T : Trait> Shape.hasTrait(): Boolean = hasTrait(T::class.java)

/** Kotlin sugar for expectTrait() check. e.g. shape.expectTrait<EnumTrait>() instead of shape.expectTrait(EnumTrait::class.java) */
inline fun <reified T : Trait> Shape.expectTrait(): T = expectTrait(T::class.java)

/** Kotlin sugar for getTrait() check. e.g. shape.getTrait<EnumTrait>() instead of shape.getTrait(EnumTrait::class.java) */
inline fun <reified T : Trait> Shape.getTrait(): T? = getTrait(T::class.java).orNull()
