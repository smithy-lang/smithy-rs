/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.validators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.DefaultTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.validation.AbstractValidator
import software.amazon.smithy.model.validation.Severity
import software.amazon.smithy.model.validation.ValidationEvent
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.targetOrSelf
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShapeForValidation
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrainedForValidation
import software.amazon.smithy.rust.codegen.server.smithy.util.isValidationMessage
import software.amazon.smithy.rust.codegen.traits.ValidationExceptionTrait

class CustomValidationExceptionValidator : AbstractValidator() {
    override fun validate(model: Model): List<ValidationEvent> {
        val events = mutableListOf<ValidationEvent>()

        model.shapes(StructureShape::class.java).filter { it.hasTrait(ValidationExceptionTrait.ID) }
            .forEach { shape ->
                // Validate that the shape also has @error trait
                if (!shape.hasTrait(ErrorTrait::class.java)) {
                    events.add(
                        ValidationEvent.builder().id("CustomValidationException.MissingErrorTrait")
                            .severity(Severity.ERROR).shape(shape)
                            .message("@validationException requires @error trait")
                            .build(),
                    )
                }

                // Validate exactly one member with @validationMessage trait (explicit) or named "message" (implicit)
                val messageFields =
                    shape.members().filter { it.isValidationMessage() }

                when (messageFields.size) {
                    0 ->
                        events.add(
                            ValidationEvent.builder().id("CustomValidationException.MissingMessageField")
                                .severity(Severity.ERROR).shape(shape)
                                .message(
                                    "@validationException requires exactly one String member named " +
                                        "\"message\" or with the @validationMessage trait",
                                ).build(),
                        )

                    1 -> {
                        val validationMessageField = messageFields.first()
                        if (!model.expectShape(validationMessageField.target).isStringShape) {
                            events.add(
                                ValidationEvent.builder().id("CustomValidationException.NonStringMessageField")
                                    .severity(Severity.ERROR).shape(shape)
                                    .message("@validationMessage field must be a String").build(),
                            )
                        }
                    }

                    else ->
                        events.add(
                            ValidationEvent.builder().id("CustomValidationException.MultipleMessageFields")
                                .severity(Severity.ERROR).shape(shape)
                                .message(
                                    "@validationException can have only one member explicitly marked with the" +
                                        "@validationMessage trait or implicitly selected via the \"message\" field name convention.",
                                ).build(),
                        )
                }

                // Validate default constructibility if it contains constrained shapes
                if (shape.canReachConstrainedShapeForValidation(model)) {
                    shape.members().forEach { member -> member.validateDefaultConstructibility(model, events) }
                }
            }

        return events
    }

    /** Validate default constructibility of the shape
     * When a validation exception occurs, the framework has to create a Rust type that represents
     * the ValidationException structure, but if that structure has fields other than 'message' and
     * 'field list', then it can't instantiate them if they don't have defaults. Later on, we will introduce
     * a mechanism for service code to be able to participate in construction of a validation exception type.
     * Until that time, we need to restrict this to default constructibility.
     */
    private fun Shape.validateDefaultConstructibility(
        model: Model,
        events: MutableList<ValidationEvent>,
    ) {
        when (this.type) {
            ShapeType.STRUCTURE -> {
                this.members().forEach { member -> member.validateDefaultConstructibility(model, events) }
            }

            ShapeType.MEMBER -> {
                // We want to check if the member's target is constrained. If so, we want the default trait to be on the
                // member.
                if (this.targetOrSelf(model).isDirectlyConstrainedForValidation() && !this.hasTrait<DefaultTrait>()) {
                    events.add(
                        ValidationEvent.builder().id("CustomValidationException.MissingDefault")
                            .severity(Severity.ERROR)
                            .message("$this must be default constructible")
                            .build(),
                    )
                }
            }

            else -> return
        }
    }
}
