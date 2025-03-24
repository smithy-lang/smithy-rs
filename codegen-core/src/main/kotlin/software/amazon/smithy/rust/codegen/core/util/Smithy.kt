/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.util

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.model.traits.TitleTrait
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait

inline fun <reified T : Shape> Model.lookup(shapeId: String): T = this.expectShape(ShapeId.from(shapeId), T::class.java)

fun OperationShape.inputShape(model: Model): StructureShape =
    // The Rust Smithy generator adds an input to all shapes automatically
    model.expectShape(this.input.get(), StructureShape::class.java)

fun OperationShape.outputShape(model: Model): StructureShape =
    // The Rust Smithy generator adds an output to all shapes automatically
    model.expectShape(this.output.get(), StructureShape::class.java)

fun StructureShape.expectMember(member: String): MemberShape =
    this.getMember(member).orElseThrow { CodegenException("$member did not exist on $this") }

fun UnionShape.expectMember(member: String): MemberShape =
    this.getMember(member).orElseThrow { CodegenException("$member did not exist on $this") }

fun StructureShape.errorMessageMember(): MemberShape? =
    this.getMember("message").or {
        this.getMember("Message")
    }.orNull()

fun StructureShape.hasStreamingMember(model: Model) = this.findStreamingMember(model) != null

fun UnionShape.hasStreamingMember(model: Model) = this.findMemberWithTrait<StreamingTrait>(model) != null

fun MemberShape.isStreaming(model: Model) = this.getMemberTrait(model, StreamingTrait::class.java).isPresent

fun UnionShape.isEventStream(): Boolean = hasTrait(StreamingTrait::class.java)

fun MemberShape.isEventStream(model: Model): Boolean =
    (model.expectShape(target) as? UnionShape)?.isEventStream() ?: false

fun MemberShape.isInputEventStream(model: Model): Boolean =
    isEventStream(model) && model.expectShape(container).hasTrait<SyntheticInputTrait>()

fun MemberShape.isOutputEventStream(model: Model): Boolean =
    isEventStream(model) && model.expectShape(container).hasTrait<SyntheticOutputTrait>()

private val unitShapeId = ShapeId.from("smithy.api#Unit")

fun Shape.isUnit(): Boolean = this.id == unitShapeId

fun MemberShape.isTargetUnit(): Boolean = this.target == unitShapeId

fun Shape.hasEventStreamMember(model: Model): Boolean = members().any { it.isEventStream(model) }

fun OperationShape.isInputEventStream(model: Model): Boolean =
    input.map { id -> model.expectShape(id).hasEventStreamMember(model) }.orElse(false)

fun OperationShape.isOutputEventStream(model: Model): Boolean =
    output.map { id -> model.expectShape(id).hasEventStreamMember(model) }.orElse(false)

fun OperationShape.isEventStream(model: Model): Boolean = isInputEventStream(model) || isOutputEventStream(model)

fun ServiceShape.hasEventStreamOperations(model: Model): Boolean =
    TopDownIndex.of(model)
        .getContainedOperations(this)
        .any { op ->
            op.isEventStream(model)
        }

fun Shape.shouldRedact(model: Model): Boolean =
    when {
        hasTrait<SensitiveTrait>() -> true
        this is MemberShape -> model.expectShape(target).shouldRedact(model)
        this is ListShape -> member.shouldRedact(model)
        this is MapShape -> key.shouldRedact(model) || value.shouldRedact(model)
        else -> false
    }

const val REDACTION = "\"*** Sensitive Data Redacted ***\""

fun Shape.redactIfNecessary(
    model: Model,
    safeToPrint: String,
): String =
    if (this.shouldRedact(model)) {
        REDACTION
    } else {
        safeToPrint
    }

/*
 * Returns the member of this structure targeted with streaming trait (if it exists).
 *
 * A structure must have at most one streaming member.
 */
fun StructureShape.findStreamingMember(model: Model): MemberShape? = this.findMemberWithTrait<StreamingTrait>(model)

inline fun <reified T : Trait> StructureShape.findMemberWithTrait(model: Model): MemberShape? =
    this.members().find { it.getMemberTrait(model, T::class.java).isPresent }

inline fun <reified T : Trait> UnionShape.findMemberWithTrait(model: Model): MemberShape? =
    this.members().find { it.getMemberTrait(model, T::class.java).isPresent }

/**
 * If is member shape returns target, otherwise returns self.
 * @param model for loading the target shape
 */
fun Shape.targetOrSelf(model: Model): Shape =
    when (this) {
        is MemberShape -> model.expectShape(this.target)
        else -> this
    }

/** Kotlin sugar for hasTrait() check. e.g. shape.hasTrait<EnumTrait>() instead of shape.hasTrait(EnumTrait::class.java) */
inline fun <reified T : Trait> Shape.hasTrait(): Boolean = hasTrait(T::class.java)

/** Kotlin sugar for expectTrait() check. e.g. shape.expectTrait<EnumTrait>() instead of shape.expectTrait(EnumTrait::class.java) */
inline fun <reified T : Trait> Shape.expectTrait(): T = expectTrait(T::class.java)

/** Kotlin sugar for getTrait() check. e.g. shape.getTrait<EnumTrait>() instead of shape.getTrait(EnumTrait::class.java) */
inline fun <reified T : Trait> Shape.getTrait(): T? = getTrait(T::class.java).orNull()

fun Shape.isPrimitive(): Boolean =
    when (this) {
        is NumberShape, is BooleanShape -> true
        else -> false
    }

/** Convert a string to a ShapeId */
fun String.shapeId() = ShapeId.from(this)

/** Returns the service name, or a default value if the service doesn't have a title trait */
fun ServiceShape.serviceNameOrDefault(default: String) = getTrait<TitleTrait>()?.value ?: default

/** Returns the SDK ID of the given service shape */
fun ServiceShape.sdkId(): String = getTrait<ServiceTrait>()?.sdkId?.lowercase()?.replace(" ", "") ?: id.getName(this)
