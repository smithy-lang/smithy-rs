/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Information needed to render a constraint trait as Rust code.
 */
data class TraitInfo(
    val tryFromCheck: Writable,
    val constraintViolationVariant: Writable,
    val asValidationExceptionField: Writable,
    val validationFunctionDefinition: (constraintViolation: Symbol, unconstrainedTypeName: String) -> Writable,
    private val testCases: List<Writable> = listOf(),
) {
    companion object {
        fun testCases(constraintsInfo: List<TraitInfo>): List<Writable> {
            return constraintsInfo.flatMap { it.testCases }
        }
    }
}

/**
 * Render the implementation of `TryFrom` for a constrained type.
 */
fun RustWriter.renderTryFrom(
    unconstrainedTypeName: String,
    constrainedTypeName: String,
    constraintViolationError: Symbol,
    constraintsInfo: List<TraitInfo>,
) {
    this.rustTemplate(
        """
        impl $constrainedTypeName {
            #{ValidationFunctions:W}
        }
        """,
        "ValidationFunctions" to
            constraintsInfo.map {
                it.validationFunctionDefinition(
                    constraintViolationError,
                    unconstrainedTypeName,
                )
            }
                .join("\n"),
    )

    this.rustTemplate(
        """
        impl #{TryFrom}<$unconstrainedTypeName> for $constrainedTypeName {
            type Error = #{ConstraintViolation};

            /// ${rustDocsTryFromMethod(constrainedTypeName, unconstrainedTypeName)}
            fn try_from(value: $unconstrainedTypeName) -> #{Result}<Self, Self::Error> {
              #{TryFromChecks:W}

              Ok(Self(value))
            }
        }
        """,
        "TryFrom" to RuntimeType.TryFrom,
        "ConstraintViolation" to constraintViolationError,
        "TryFromChecks" to constraintsInfo.map { it.tryFromCheck }.join("\n"),
        *RuntimeType.preludeScope,
    )
}
