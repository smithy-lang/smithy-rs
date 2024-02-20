package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.PatternTrait
import software.amazon.smithy.rust.codegen.server.smithy.generators.idWithHashEscaped

fun PatternTrait.validationErrorMessage() =
    "Value at '{}' failed to satisfy constraint: Member must satisfy regular expression pattern: ${this.patternDescription()}"

fun PatternTrait.shapeConstraintViolationDisplayMessage(shape: Shape) =
    "Value provided for `${shape.idWithHashEscaped()}` failed to satisfy the constraint: Member must match the regular expression pattern: ${this.patternDescription()}"

// A '#' character in the pattern must be replaced with "##" for the message to be usable
// within `rustTemplate`, as it interpolates anything prefixed with '#'. Additionally,
// the `toString()` representation of a regular expression that includes two backslashes yields
// only one backslash. For instance, the `toString()` of a `PatternTrait` that represents
// `@pattern("\\d")` in Smithy, yields "\d". Writing this directly in generated code as-is leads
// to an "unknown character escape" error in Rust.
fun PatternTrait.patternDescription() =
    this.pattern.toString()
        .replace("#", "##")
        .replace("\\", "\\\\")
