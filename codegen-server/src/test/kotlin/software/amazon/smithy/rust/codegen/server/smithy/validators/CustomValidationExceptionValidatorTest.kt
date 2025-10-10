/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.validators

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.collections.shouldHaveSize
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.validation.Severity
import software.amazon.smithy.model.validation.ValidatedResultException
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class CustomValidationExceptionValidatorTest {
    @Test
    fun `should error when validationException lacks error trait`() {
        val exception =
            shouldThrow<ValidatedResultException> {
                """
                namespace test
                use smithy.rust.codegen.traits#validationException
                use smithy.rust.codegen.traits#validationMessage

                @validationException
                structure ValidationError {
                    @validationMessage
                    message: String
                }
                """.asSmithyModel(smithyVersion = "2")
            }
        val events = exception.validationEvents.filter { it.severity == Severity.ERROR }

        events shouldHaveSize 1
        events[0].shapeId.get() shouldBe ShapeId.from("test#ValidationError")
        events[0].id shouldBe "CustomValidationException.MissingErrorTrait"
    }

    @Test
    fun `should error when validationException has no validationMessage field`() {
        val exception =
            shouldThrow<ValidatedResultException> {
                """
                namespace test
                use smithy.rust.codegen.traits#validationException

                @validationException
                @error("client")
                structure ValidationError {
                    code: String
                }
                """.asSmithyModel(smithyVersion = "2")
            }
        val events = exception.validationEvents.filter { it.severity == Severity.ERROR }

        events shouldHaveSize 1
        events[0].shapeId.get() shouldBe ShapeId.from("test#ValidationError")
        events[0].id shouldBe "CustomValidationException.MissingMessageField"
    }

    @Test
    fun `should error when validationException has multiple explicit validationMessage fields`() {
        val exception =
            shouldThrow<ValidatedResultException> {
                """
                namespace test
                use smithy.rust.codegen.traits#validationException
                use smithy.rust.codegen.traits#validationMessage

                @validationException
                @error("client")
                structure ValidationError {
                    @validationMessage
                    message: String,
                    @validationMessage
                    details: String
                }
                """.asSmithyModel(smithyVersion = "2")
            }
        val events = exception.validationEvents.filter { it.severity == Severity.ERROR }

        events shouldHaveSize 1
        events[0].shapeId.get() shouldBe ShapeId.from("test#ValidationError")
        events[0].id shouldBe "CustomValidationException.MultipleMessageFields"
    }

    @Test
    fun `should error when validationException has explicit validationMessage and implicit message fields`() {
        val exception =
            shouldThrow<ValidatedResultException> {
                """
                namespace test
                use smithy.rust.codegen.traits#validationException
                use smithy.rust.codegen.traits#validationMessage

                @validationException
                @error("client")
                structure ValidationError {
                    message: String,
                    @validationMessage
                    details: String,
                }
                """.asSmithyModel(smithyVersion = "2")
            }
        val events = exception.validationEvents.filter { it.severity == Severity.ERROR }

        events shouldHaveSize 1
        events[0].shapeId.get() shouldBe ShapeId.from("test#ValidationError")
        events[0].id shouldBe "CustomValidationException.MultipleMessageFields"
    }

    @Test
    fun `should error when constrained shape lacks default trait`() {
        val exception =
            shouldThrow<ValidatedResultException> {
                """
                namespace test
                use smithy.rust.codegen.traits#validationException
                use smithy.rust.codegen.traits#validationMessage

                @validationException
                @error("client")
                structure ValidationError {
                    @validationMessage
                    message: String,
                    constrainedField: ConstrainedString
                }

                @length(min: 1, max: 10)
                string ConstrainedString
                """.asSmithyModel(smithyVersion = "2")
            }
        val events = exception.validationEvents.filter { it.severity == Severity.ERROR }

        events shouldHaveSize 1
        events[0].id shouldBe "CustomValidationException.MissingDefault"
    }

    @Test
    fun `should pass validation for properly configured validationException`() {
        """
        namespace test
        use smithy.rust.codegen.traits#validationException
        use smithy.rust.codegen.traits#validationMessage

        @validationException
        @error("client")
        structure ValidationError {
            @validationMessage
            message: String
        }
        """.asSmithyModel(smithyVersion = "2")
    }

    @Test
    fun `should pass validation for validationException with constrained shape having default`() {
        """
        namespace test
        use smithy.rust.codegen.traits#validationException
        use smithy.rust.codegen.traits#validationMessage

        @validationException
        @error("client")
        structure ValidationError {
            @validationMessage
            message: String,
            @default("default")
            constrainedField: ConstrainedString
        }

        @length(min: 1, max: 10)
        @default("default")
        string ConstrainedString
        """.asSmithyModel(smithyVersion = "2")
    }
}
