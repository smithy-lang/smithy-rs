/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.PatternTrait
import software.amazon.smithy.model.validation.AbstractValidator
import software.amazon.smithy.model.validation.ValidationEvent
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait

class PatternTraitEscapedSpecialCharsValidator : AbstractValidator() {
    private val specialCharsWithEscapes = mapOf(
        '\b' to "\\b",
        '\u000C' to "\\f",
        '\n' to "\\n",
        '\r' to "\\r",
        '\t' to "\\t",
    )
    private val specialChars = specialCharsWithEscapes.keys

    override fun validate(model: Model): List<ValidationEvent> {
        val shapes = model.getStringShapesWithTrait(PatternTrait::class.java) +
            model.getMemberShapesWithTrait(PatternTrait::class.java)
        return shapes
            .filter { shape -> checkMisuse(shape) }
            .map { shape -> makeError(shape) }
            .toList()
    }

    private fun makeError(shape: Shape): ValidationEvent {
        val pattern = shape.expectTrait<PatternTrait>()
        val replacement = pattern.pattern.toString()
            .map { specialCharsWithEscapes.getOrElse(it) { it.toString() } }
            .joinToString("")
            .dq()
        val message =
            """
            Non-escaped special characters used inside `@pattern`.
            You must escape them: `@pattern($replacement)`.
            See https://github.com/awslabs/smithy-rs/issues/2508 for more details.
            """.trimIndent()
        return error(shape, pattern, message)
    }

    private fun checkMisuse(shape: Shape): Boolean {
        val pattern = shape.expectTrait<PatternTrait>().pattern.pattern()
        return pattern.any(specialChars::contains)
    }
}
