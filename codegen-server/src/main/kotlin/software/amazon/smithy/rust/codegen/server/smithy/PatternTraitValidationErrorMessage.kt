package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.PatternTrait

fun PatternTrait.validationErrorMessage() =
    "Value at '{}' failed to satisfy constraint: Member must satisfy regular expression pattern: ${this.patternDescription()}"

fun PatternTrait.shapeConstraintViolationDisplayMessage(shape: Shape) =
    """
    Value '{}' provided for `${shape.id.toString().replace("#", "##")}` failed to satisfy the constraint: 
    Member must match the regular expression pattern: ${this.patternDescription()}
    """.trimIndent()

// A '#' character in the pattern needs to be replaced with "##" for the message to be used
// as part of `rustTemplate`.
fun PatternTrait.patternDescription() =
    this.pattern.toString().replace("#", "##")
