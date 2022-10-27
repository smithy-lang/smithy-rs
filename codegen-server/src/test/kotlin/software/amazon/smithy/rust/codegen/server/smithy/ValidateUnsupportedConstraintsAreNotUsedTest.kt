/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.inspectors.forSome
import io.kotest.inspectors.shouldForAll
import io.kotest.matchers.collections.shouldHaveAtLeastSize
import io.kotest.matchers.collections.shouldHaveSize
import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
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
        validationResult.messages[0].message shouldContain "Operation test#TestOperation takes in input that is constrained"
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

        validationResult.messages shouldHaveSize 2
        validationResult.messages.forSome {
            it.message shouldContain
                """
                The blob shape `test#StreamingBlob` has both the `smithy.api#length` and `smithy.api#streaming` constraint traits attached.
                It is unclear what the semantics for streaming blob shapes are.
                """.trimIndent().replace("\n", " ")
        }
    }

    @Test
    fun `it should detect when constraint traits in event streams are used`() {
        val model =
            """
            $baseModel
            
            structure TestInputOutput {
                eventStream: EventStream
            }
            
            @streaming
            union EventStream {
                message: Message
            }
            
            structure Message {
                lengthString: LengthString
            }
            
            @length(min: 1)
            string LengthString
            """.asSmithyModel()
        val validationResult = validateModel(model)

        validationResult.messages shouldHaveSize 1
        validationResult.messages[0].message shouldContain
            """
            The string shape `test#LengthString` has the constraint trait `smithy.api#length` attached.
            This shape is also part of an event stream; it is unclear what the semantics for constrained shapes in event streams are.
            """.trimIndent().replace("\n", " ")
    }

    @Test
    fun `it should detect when the length trait on collection shapes or on blob shapes is used`() {
        val model =
            """
            $baseModel
            
            structure TestInputOutput {
                collection: LengthCollection,
                blob: LengthBlob
            }
            
            @length(min: 1)
            list LengthCollection {
                member: String
            }
            
            @length(min: 1)
            blob LengthBlob
            """.asSmithyModel()
        val validationResult = validateModel(model)

        validationResult.messages shouldHaveSize 2
        validationResult.messages.forSome { it.message shouldContain "The list shape `test#LengthCollection` has the constraint trait `smithy.api#length` attached" }
        validationResult.messages.forSome { it.message shouldContain "The blob shape `test#LengthBlob` has the constraint trait `smithy.api#length` attached" }
    }

    @Test
    fun `it should detect when the pattern trait on string shapes is used`() {
        val model =
            """
            $baseModel
            
            structure TestInputOutput {
                patternString: PatternString
            }
            
            @pattern("^[A-Za-z]+$")
            string PatternString
            """.asSmithyModel()
        val validationResult = validateModel(model)

        validationResult.messages shouldHaveSize 1
        validationResult.messages[0].message shouldContain "The string shape `test#PatternString` has the constraint trait `smithy.api#pattern` attached"
    }

    @Test
    fun `it should detect when the range trait is used`() {
        val model =
            """
            $baseModel
            
            structure TestInputOutput {
                rangeInteger: RangeInteger
            }
            
            @range(min: 1)
            integer RangeInteger
            """.asSmithyModel()
        val validationResult = validateModel(model)

        validationResult.messages shouldHaveSize 1
        validationResult.messages[0].message shouldContain "The integer shape `test#RangeInteger` has the constraint trait `smithy.api#range` attached"
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
