/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

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
// passing the `toString()` representation of a regular expression directly to a function
// that interpolates (e.g., println!, format!) can cause an error if the string contains
// interpolation characters. Therefore, these characters must be escaped.
fun PatternTrait.patternDescription() =
    this.pattern.toString()
        .replace("#", "##")
        .replace("\\", "\\\\")
        .replace("{","{{")
        .replace("}","}}")
