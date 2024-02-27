/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.createTestInlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator

object TestUtility {
    fun generateIsDisplay() = writable {
        rustTemplate(
            """
            fn is_display<T : #{Display}>(_t: &T) { }
            """,
        "Display" to RuntimeType.Display,)
    }

    fun generateIsError() = writable {
        rustTemplate(
            """
            fn is_error<T : #{Error}>(_t: &T) { }
            """,
            "Error" to RuntimeType.StdError
        )
    }

    fun renderConstrainedString(
        codegenContext: ServerCodegenContext,
        writer: RustWriter,
        constrainedStringShape: StringShape,
    ) {
        val validationExceptionConversionGenerator = SmithyValidationExceptionConversionGenerator(codegenContext)
        ConstrainedStringGenerator(
            codegenContext,
            writer.createTestInlineModuleCreator(),
            writer,
            constrainedStringShape,
            validationExceptionConversionGenerator,
        ).render()
    }
}
