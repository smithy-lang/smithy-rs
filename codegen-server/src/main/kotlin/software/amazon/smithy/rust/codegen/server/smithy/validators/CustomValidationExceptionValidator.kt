package software.amazon.smithy.rust.codegen.server.smithy.validators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.DefaultTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.validation.AbstractValidator
import software.amazon.smithy.model.validation.Severity
import software.amazon.smithy.model.validation.ValidationEvent
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShapeForValidation
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrainedForValidation
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.server.traits.ValidationExceptionTrait
import software.amazon.smithy.rust.codegen.server.traits.ValidationMessageTrait

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

                // Validate exactly one @validationMessage field
                val messageFields = shape.members().filter { it.hasTrait(ValidationMessageTrait.ID) }

                when (messageFields.size) {
                    0 -> events.add(
                        ValidationEvent.builder().id("CustomValidationException.MissingMessageField")
                            .severity(Severity.ERROR).shape(shape)
                            .message("@validationException requires exactly one @validationMessage field").build(),
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

                    else -> events.add(
                        ValidationEvent.builder().id("CustomValidationException.MultipleMessageFields")
                            .severity(Severity.ERROR).shape(shape)
                            .message("@validationException can have only one @validationMessage field").build(),
                    )
                }

                // Validate default constructibility if it contains constrained shapes
                if (shape.canReachConstrainedShapeForValidation(model)) {
                    shape.validateDefaultConstructibility(model, events)
                }
            }

        return events
    }

    private fun Shape.validateDefaultConstructibility(
        model: Model,
        events: MutableList<ValidationEvent>,
    ) {
        val shapes = DirectedWalker(model).walkShapes(this)
        for (shape in shapes) {
            if (shape == this) continue
            if (shape.isDirectlyConstrainedForValidation() && !shape.hasTrait<DefaultTrait>()) {
                events.add(
                    ValidationEvent.builder().id("CustomValidationException.MissingDefault")
                        .severity(Severity.ERROR)
                        .message("$shape must be default constructible")
                        .build(),
                )
            }
        }
    }
}
