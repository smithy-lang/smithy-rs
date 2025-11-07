/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.LogMessage
import software.amazon.smithy.rust.codegen.server.smithy.ValidationResult
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class PostprocessValidationExceptionNotAttachedErrorMessageDecoratorTest {
    @Test
    fun `validation exception not attached error message is postprocessed if decorator is registered`() {
        val model =
            """
            namespace test
            use aws.protocols#restJson1

            @restJson1
            service TestService {
                operations: ["ConstrainedOperation"],
            }

            operation ConstrainedOperation {
                input: ConstrainedOperationInput
            }

            structure ConstrainedOperationInput {
                @required
                requiredString: String
            }
            """.asSmithyModel()

        val validationExceptionNotAttachedErrorMessageDummyPostprocessorDecorator =
            object : ServerCodegenDecorator {
                override val name: String
                    get() = "ValidationExceptionNotAttachedErrorMessageDummyPostprocessorDecorator"
                override val order: Byte
                    get() = 69

                override fun postprocessValidationExceptionNotAttachedErrorMessage(
                    validationResult: ValidationResult,
                ): ValidationResult {
                    check(validationResult.messages.size == 1)

                    val level = validationResult.messages.first().level
                    val message =
                        """
                        ${validationResult.messages.first().message}

                        There are three things all wise men fear: the sea in storm, a night with no moon, and the anger of a gentle man.
                        """

                    return validationResult.copy(messages = listOf(LogMessage(level, message)))
                }
            }

        val exception =
            assertThrows<CodegenException> {
                serverIntegrationTest(
                    model,
                    additionalDecorators = listOf(validationExceptionNotAttachedErrorMessageDummyPostprocessorDecorator),
                    testCoverage = HttpTestType.AS_CONFIGURED,
                )
            }
        val exceptionCause = (exception.cause!! as ValidationResult)
        exceptionCause.messages.size shouldBe 1
        exceptionCause.messages.first().message shouldContain "There are three things all wise men fear: the sea in storm, a night with no moon, and the anger of a gentle man."
    }
}
