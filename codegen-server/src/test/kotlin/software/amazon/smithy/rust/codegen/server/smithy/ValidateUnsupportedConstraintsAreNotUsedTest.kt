/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.inspectors.forOne
import io.kotest.inspectors.shouldForAll
import io.kotest.matchers.collections.shouldHaveAtLeastSize
import io.kotest.matchers.collections.shouldHaveSize
import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import io.kotest.matchers.string.shouldNotContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.core.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup
import java.util.logging.Level

internal class ValidateUnsupportedConstraintsAreNotUsedTest {
    private val baseModel =
        """
        namespace test

        service TestService {
            version: "123",
            operations: [TestOperation]
        }

        operation TestOperation {
            input: TestInputOutput,
            output: TestInputOutput,
        }
        """

    private fun validateModel(model: Model, serverCodegenConfig: ServerCodegenConfig = ServerCodegenConfig()): ValidationResult {
        val service = model.lookup<ServiceShape>("test#TestService")
        return validateUnsupportedConstraints(model, service, serverCodegenConfig)
    }

    @Test
    fun `it should detect when an operation with constrained input but that does not have ValidationException attached in errors`() {
        val model =
            """
            $baseModel

            structure TestInputOutput {
                @required
                requiredString: String
            }
            """.asSmithyModel()
        val service = model.lookup<ServiceShape>("test#TestService")
        val validationResult = validateOperationsWithConstrainedInputHaveValidationExceptionAttached(model, service)

        validationResult.messages shouldHaveSize 1

        // Asserts the exact message, to ensure the formatting is appropriate.
        validationResult.messages[0].message shouldBe """
            Operation test#TestOperation takes in input that is constrained (https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html), and as such can fail with a validation exception. You must model this behavior in the operation shape in your model file.
            ```smithy
            use smithy.framework#ValidationException

            operation TestOperation {
                ...
                errors: [..., ValidationException] // <-- Add this.
            }
            ```
        """.trimIndent()
    }

    @Test
    fun `it should detect when unsupported constraint traits on member shapes are used`() {
        val model =
            """
            $baseModel

            structure TestInputOutput {
                @length(min: 1, max: 69)
                lengthString: String
            }
            """.asSmithyModel()
        val validationResult = validateModel(model)

        validationResult.messages shouldHaveSize 1
        validationResult.messages[0].message shouldContain "The member shape `test#TestInputOutput\$lengthString` has the constraint trait `smithy.api#length` attached"
    }

    @Test
    fun `it should not detect when the required trait on a member shape is used`() {
        val model =
            """
            $baseModel

            structure TestInputOutput {
                @required
                string: String
            }
            """.asSmithyModel()
        val validationResult = validateModel(model)

        validationResult.messages shouldHaveSize 0
    }

    private val constraintTraitOnStreamingBlobShapeModel =
        """
        $baseModel

        structure TestInputOutput {
            @required
            streamingBlob: StreamingBlob
        }

        @streaming
        @length(min: 69)
        blob StreamingBlob
        """.asSmithyModel()

    @Test
    fun `it should detect when constraint traits on streaming blob shapes are used`() {
        val validationResult = validateModel(constraintTraitOnStreamingBlobShapeModel)

        validationResult.messages shouldHaveSize 1
        validationResult.messages[0].message shouldContain """
            The blob shape `test#StreamingBlob` has both the `smithy.api#length` and `smithy.api#streaming` constraint traits attached.
            It is unclear what the semantics for streaming blob shapes are.
        """.trimIndent().replace("\n", " ")
    }

    val constrainedShapesInEventStreamModel =
        """
        $baseModel

        structure TestInputOutput {
            eventStream: EventStream
        }

        @streaming
        union EventStream {
            message: Message,
            error: Error
        }

        structure Message {
            lengthString: LengthString
        }

        structure Error {
            @required
            message: String
        }

        @length(min: 1)
        string LengthString
        """.asSmithyModel()

    @Test
    fun `it should detect when constraint traits in event streams are used`() {
        val validationResult = validateModel(EventStreamNormalizer.transform(constrainedShapesInEventStreamModel))

        validationResult.messages shouldHaveSize 2
        validationResult.messages.forOne {
            it.message shouldContain
                """
                The string shape `test#LengthString` has the constraint trait `smithy.api#length` attached.
                This shape is also part of an event stream; it is unclear what the semantics for constrained shapes in event streams are.
                Please remove the trait from the shape to synthesize your model.
                """.trimIndent().replace("\n", " ")
            it.message shouldNotContain "If you want to go ahead and generate the server SDK ignoring unsupported constraint traits"
        }
        validationResult.messages.forOne {
            it.message shouldContain
                """
                The member shape `test#Error${"$"}message` has the constraint trait `smithy.api#required` attached.
                This shape is also part of an event stream; it is unclear what the semantics for constrained shapes in event streams are.
                Please remove the trait from the shape to synthesize your model.
                """.trimIndent().replace("\n", " ")
            it.message shouldNotContain "If you want to go ahead and generate the server SDK ignoring unsupported constraint traits"
        }
    }

    @Test
    fun `it should abort when constraint traits in event streams are used, despite opting into ignoreUnsupportedConstraintTraits`() {
        val validationResult = validateModel(
            EventStreamNormalizer.transform(constrainedShapesInEventStreamModel),
            ServerCodegenConfig().copy(ignoreUnsupportedConstraints = true),
        )

        validationResult.shouldAbort shouldBe true
    }

    @Test
    fun `it should abort when ignoreUnsupportedConstraints is false and unsupported constraints are used`() {
        val validationResult = validateModel(constraintTraitOnStreamingBlobShapeModel, ServerCodegenConfig())

        validationResult.messages shouldHaveAtLeastSize 1
        validationResult.shouldAbort shouldBe true
    }

    @Test
    fun `it should not abort when ignoreUnsupportedConstraints is true and unsupported constraints are used`() {
        val validationResult = validateModel(
            constraintTraitOnStreamingBlobShapeModel,
            ServerCodegenConfig().copy(ignoreUnsupportedConstraints = true),
        )

        validationResult.messages shouldHaveAtLeastSize 1
        validationResult.shouldAbort shouldBe false
    }

    @Test
    fun `it should set log level to error when ignoreUnsupportedConstraints is false and unsupported constraints are used`() {
        val validationResult = validateModel(constraintTraitOnStreamingBlobShapeModel, ServerCodegenConfig())

        validationResult.messages shouldHaveAtLeastSize 1
        validationResult.messages.shouldForAll { it.level shouldBe Level.SEVERE }
    }

    @Test
    fun `it should set log level to warn when ignoreUnsupportedConstraints is true and unsupported constraints are used`() {
        val validationResult = validateModel(
            constraintTraitOnStreamingBlobShapeModel,
            ServerCodegenConfig().copy(ignoreUnsupportedConstraints = true),
        )

        validationResult.messages shouldHaveAtLeastSize 1
        validationResult.messages.shouldForAll { it.level shouldBe Level.WARNING }
    }
}
