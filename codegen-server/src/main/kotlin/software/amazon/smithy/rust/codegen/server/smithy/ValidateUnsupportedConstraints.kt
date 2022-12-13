/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.model.traits.RangeTrait
import software.amazon.smithy.model.traits.RequiredTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.model.traits.UniqueItemsTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticEventStreamUnionTrait
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull
import java.util.logging.Level

private sealed class UnsupportedConstraintMessageKind {
    private val constraintTraitsUberIssue = "https://github.com/awslabs/smithy-rs/issues/1401"

    fun intoLogMessage(ignoreUnsupportedConstraints: Boolean): LogMessage {
        fun buildMessage(intro: String, willSupport: Boolean, trackingIssue: String) =
            """
            $intro
            This is not supported in the smithy-rs server SDK.
            ${if (willSupport) "It will be supported in the future." else ""}
            See the tracking issue ($trackingIssue).
            If you want to go ahead and generate the server SDK ignoring unsupported constraint traits, set the key `ignoreUnsupportedConstraints`
            inside the `runtimeConfig.codegenConfig` JSON object in your `smithy-build.json` to `true`.
            """.trimIndent().replace("\n", " ")

        fun buildMessageShapeHasUnsupportedConstraintTrait(
            shape: Shape,
            constraintTrait: Trait,
            trackingIssue: String,
        ) =
            buildMessage(
                "The ${shape.type} shape `${shape.id}` has the constraint trait `${constraintTrait.toShapeId()}` attached.",
                willSupport = true,
                trackingIssue,
            )

        val level = if (ignoreUnsupportedConstraints) Level.WARNING else Level.SEVERE

        return when (this) {
            is UnsupportedConstraintOnMemberShape -> LogMessage(
                level,
                buildMessageShapeHasUnsupportedConstraintTrait(shape, constraintTrait, constraintTraitsUberIssue),
            )

            is UnsupportedConstraintOnShapeReachableViaAnEventStream -> LogMessage(
                level,
                buildMessage(
                    """
                    The ${shape.type} shape `${shape.id}` has the constraint trait `${constraintTrait.toShapeId()}` attached.
                    This shape is also part of an event stream; it is unclear what the semantics for constrained shapes in event streams are.
                    """.trimIndent().replace("\n", " "),
                    willSupport = false,
                    "https://github.com/awslabs/smithy/issues/1388",
                ),
            )

            is UnsupportedLengthTraitOnStreamingBlobShape -> LogMessage(
                level,
                buildMessage(
                    """
                    The ${shape.type} shape `${shape.id}` has both the `${lengthTrait.toShapeId()}` and `${streamingTrait.toShapeId()}` constraint traits attached.
                    It is unclear what the semantics for streaming blob shapes are.
                    """.trimIndent().replace("\n", " "),
                    willSupport = false,
                    "https://github.com/awslabs/smithy/issues/1389",
                ),
            )

            is UnsupportedLengthTraitOnBlobShape -> LogMessage(
                level,
                buildMessageShapeHasUnsupportedConstraintTrait(shape, lengthTrait, constraintTraitsUberIssue),
            )

            is UnsupportedRangeTraitOnShape -> LogMessage(
                level,
                buildMessageShapeHasUnsupportedConstraintTrait(shape, rangeTrait, constraintTraitsUberIssue),
            )

            is UnsupportedUniqueItemsTraitOnShape -> LogMessage(
                level,
                buildMessageShapeHasUnsupportedConstraintTrait(shape, uniqueItemsTrait, constraintTraitsUberIssue),
            )
        }
    }
}

private data class OperationWithConstrainedInputWithoutValidationException(val shape: OperationShape)
private data class UnsupportedConstraintOnMemberShape(val shape: MemberShape, val constraintTrait: Trait) :
    UnsupportedConstraintMessageKind()

private data class UnsupportedConstraintOnShapeReachableViaAnEventStream(val shape: Shape, val constraintTrait: Trait) :
    UnsupportedConstraintMessageKind()

private data class UnsupportedLengthTraitOnStreamingBlobShape(
    val shape: BlobShape,
    val lengthTrait: LengthTrait,
    val streamingTrait: StreamingTrait,
) : UnsupportedConstraintMessageKind()

private data class UnsupportedLengthTraitOnBlobShape(val shape: Shape, val lengthTrait: LengthTrait) :
    UnsupportedConstraintMessageKind()

private data class UnsupportedRangeTraitOnShape(val shape: Shape, val rangeTrait: RangeTrait) :
    UnsupportedConstraintMessageKind()

private data class UnsupportedUniqueItemsTraitOnShape(val shape: Shape, val uniqueItemsTrait: UniqueItemsTrait) :
    UnsupportedConstraintMessageKind()

data class LogMessage(val level: Level, val message: String)
data class ValidationResult(val shouldAbort: Boolean, val messages: List<LogMessage>)

private val unsupportedConstraintsOnMemberShapes = allConstraintTraits - RequiredTrait::class.java

fun validateOperationsWithConstrainedInputHaveValidationExceptionAttached(
    model: Model,
    service: ServiceShape,
): ValidationResult {
    // Traverse the model and error out if an operation uses constrained input, but it does not have
    // `ValidationException` attached in `errors`. https://github.com/awslabs/smithy-rs/pull/1199#discussion_r809424783
    // TODO(https://github.com/awslabs/smithy-rs/issues/1401): This check will go away once we add support for
    //  `disableDefaultValidation` set to `true`, allowing service owners to map from constraint violations to operation errors.
    val walker = Walker(model)
    val operationsWithConstrainedInputWithoutValidationExceptionSet = walker.walkShapes(service)
        .filterIsInstance<OperationShape>()
        .asSequence()
        .filter { operationShape ->
            // Walk the shapes reachable via this operation input.
            walker.walkShapes(operationShape.inputShape(model))
                .any { it is SetShape || it is EnumShape || it.hasConstraintTrait() }
        }
        .filter { !it.errors.contains(ShapeId.from("smithy.framework#ValidationException")) }
        .map { OperationWithConstrainedInputWithoutValidationException(it) }
        .toSet()

    val messages =
        operationsWithConstrainedInputWithoutValidationExceptionSet.map {
            LogMessage(
                Level.SEVERE,
                """
                Operation ${it.shape.id} takes in input that is constrained
                (https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html), and as such can fail with a validation
                exception. You must model this behavior in the operation shape in your model file.
                """.trimIndent().replace("\n", "") +
                    """

                    ```smithy
                    use smithy.framework#ValidationException

                    operation ${it.shape.id.name} {
                        ...
                        errors: [..., ValidationException] // <-- Add this.
                    }
                    ```
                    """.trimIndent(),
            )
        }

    return ValidationResult(shouldAbort = messages.any { it.level == Level.SEVERE }, messages)
}

fun validateUnsupportedConstraints(
    model: Model,
    service: ServiceShape,
    codegenConfig: ServerCodegenConfig,
): ValidationResult {
    // Traverse the model and error out if:
    val walker = Walker(model)

    // 1. Constraint traits on member shapes are used. [Constraint trait precedence] has not been implemented yet.
    // TODO(https://github.com/awslabs/smithy-rs/issues/1401)
    // [Constraint trait precedence]: https://awslabs.github.io/smithy/2.0/spec/model.html#applying-traits
    val unsupportedConstraintOnMemberShapeSet = walker
        .walkShapes(service)
        .asSequence()
        .filterIsInstance<MemberShape>()
        .filterMapShapesToTraits(unsupportedConstraintsOnMemberShapes)
        .map { (shape, trait) -> UnsupportedConstraintOnMemberShape(shape as MemberShape, trait) }
        .toSet()

    // 2. Constraint traits on streaming blob shapes are used. Their semantics are unclear.
    // TODO(https://github.com/awslabs/smithy/issues/1389)
    val unsupportedLengthTraitOnStreamingBlobShapeSet = walker
        .walkShapes(service)
        .asSequence()
        .filterIsInstance<BlobShape>()
        .filter { it.hasTrait<LengthTrait>() && it.hasTrait<StreamingTrait>() }
        .map { UnsupportedLengthTraitOnStreamingBlobShape(it, it.expectTrait(), it.expectTrait()) }
        .toSet()

    // 3. Constraint traits in event streams are used. Their semantics are unclear.
    // TODO(https://github.com/awslabs/smithy/issues/1388)
    val eventStreamShapes = walker
        .walkShapes(service)
        .asSequence()
        .filter { it.hasTrait<SyntheticEventStreamUnionTrait>() }
    val unsupportedConstraintOnNonErrorShapeReachableViaAnEventStreamSet = eventStreamShapes
        .flatMap { walker.walkShapes(it) }
        .filterMapShapesToTraits(allConstraintTraits)
        .map { (shape, trait) -> UnsupportedConstraintOnShapeReachableViaAnEventStream(shape, trait) }
        .toSet()
    val eventStreamErrors = eventStreamShapes.map { it.expectTrait<SyntheticEventStreamUnionTrait>() }.map { it.errorMembers }
    val unsupportedConstraintErrorShapeReachableViaAnEventStreamSet = eventStreamErrors
        .flatMap { it }
        .flatMap { walker.walkShapes(it) }
        .filterMapShapesToTraits(allConstraintTraits)
        .map { (shape, trait) -> UnsupportedConstraintOnShapeReachableViaAnEventStream(shape, trait) }
        .toSet()
    val unsupportedConstraintShapeReachableViaAnEventStreamSet =
        unsupportedConstraintOnNonErrorShapeReachableViaAnEventStreamSet + unsupportedConstraintErrorShapeReachableViaAnEventStreamSet

    // 4. Length trait on blob shapes is used. It has not been implemented yet.
    // TODO(https://github.com/awslabs/smithy-rs/issues/1401)
    val unsupportedLengthTraitOnBlobShapeSet = walker
        .walkShapes(service)
        .asSequence()
        .filterIsInstance<BlobShape>()
        .mapNotNull {
            it.getTrait<LengthTrait>()?.let { trait -> UnsupportedLengthTraitOnBlobShape(it, trait) }
        }
        .toSet()

    // 5. Range trait used on unsupported shapes.
    // TODO(https://github.com/awslabs/smithy-rs/issues/1401)
    val unsupportedRangeTraitOnShapeSet = walker
        .walkShapes(service)
        .asSequence()
        .filterNot { it is IntegerShape || it is ShortShape || it is LongShape || it is ByteShape }
        .filterMapShapesToTraits(setOf(RangeTrait::class.java))
        .map { (shape, rangeTrait) -> UnsupportedRangeTraitOnShape(shape, rangeTrait as RangeTrait) }
        .toSet()

    // 6. UniqueItems trait on any shape is used. It has not been implemented yet.
    // TODO(https://github.com/awslabs/smithy-rs/issues/1401)
    val unsupportedUniqueItemsTraitOnShapeSet = walker
        .walkShapes(service)
        .asSequence()
        .filterMapShapesToTraits(setOf(UniqueItemsTrait::class.java))
        .map { (shape, uniqueItemsTrait) ->
            UnsupportedUniqueItemsTraitOnShape(
                shape,
                uniqueItemsTrait as UniqueItemsTrait,
            )
        }
        .toSet()

    val messages =
        unsupportedConstraintOnMemberShapeSet.map { it.intoLogMessage(codegenConfig.ignoreUnsupportedConstraints) } +
            unsupportedLengthTraitOnStreamingBlobShapeSet.map { it.intoLogMessage(codegenConfig.ignoreUnsupportedConstraints) } +
            unsupportedConstraintShapeReachableViaAnEventStreamSet.map { it.intoLogMessage(codegenConfig.ignoreUnsupportedConstraints) } +
            unsupportedLengthTraitOnBlobShapeSet.map { it.intoLogMessage(codegenConfig.ignoreUnsupportedConstraints) } +
            unsupportedRangeTraitOnShapeSet.map { it.intoLogMessage(codegenConfig.ignoreUnsupportedConstraints) } +
            unsupportedUniqueItemsTraitOnShapeSet.map { it.intoLogMessage(codegenConfig.ignoreUnsupportedConstraints) }

    return ValidationResult(shouldAbort = messages.any { it.level == Level.SEVERE }, messages)
}

/**
 * Returns a sequence over pairs `(shape, trait)`.
 * The returned sequence contains one pair per shape in the input iterable that has attached a trait contained in  [traits].
 */
private fun Sequence<Shape>.filterMapShapesToTraits(traits: Set<Class<out Trait>>): Sequence<Pair<Shape, Trait>> =
    this.map { shape -> shape to traits.mapNotNull { shape.getTrait(it).orNull() } }
        .flatMap { (shape, traits) -> traits.map { shape to it } }
