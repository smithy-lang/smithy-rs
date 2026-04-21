/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.booleans.shouldBeFalse
import io.kotest.matchers.booleans.shouldBeTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Tests whether the server `codegen` flag `addValidationExceptionToConstrainedOperations` works as expected.
 */
internal class AddValidationExceptionToConstrainedOperationsTest {
    private val validationExceptionShapeId = SmithyValidationExceptionConversionGenerator.SHAPE_ID

    private val testModelWithValidationExceptionImported =
        """
        namespace test

        use smithy.framework#ValidationException
        use aws.protocols#restJson1
        use aws.api#data

        @restJson1
        service ConstrainedService {
            operations: [SampleOperationWithValidation, SampleOperationWithoutValidation]
        }

        @http(uri: "/anOperationWithValidation", method: "POST")
        operation SampleOperationWithValidation {
            output: SampleInputOutput
            input: SampleInputOutput
            errors: [ValidationException, ErrorWithMemberConstraint]
        }
        @http(uri: "/anOperationWithoutValidation", method: "POST")
        operation SampleOperationWithoutValidation {
            output: SampleInputOutput
            input: SampleInputOutput
            errors: []
        }
        structure SampleInputOutput {
            constrainedInteger : RangedInteger
            @range(min: 2, max:100)
            constrainedMemberInteger : RangedInteger
            patternString : PatternString
        }
        @pattern("^[a-m]+${'$'}")
        string PatternString
        @range(min: 0, max:1000)
        integer RangedInteger

        @error("server")
        structure ErrorWithMemberConstraint {
            @range(min: 100, max: 999)
            statusCode: Integer
        }
        """.asSmithyModel(smithyVersion = "2")

    /**
     * Verify the test model is set up correctly: `SampleOperationWithoutValidation` and
     * the service should not have `smithy.framework#ValidationException` in the errors.
     */
    private fun verifyTestModelDoesNotHaveValidationException() {
        val operation =
            testModelWithValidationExceptionImported.expectShape(
                ShapeId.from("test#SampleOperationWithoutValidation"),
                OperationShape::class.java,
            )
        operation.errors.contains(validationExceptionShapeId).shouldBeFalse()

        val service =
            testModelWithValidationExceptionImported.expectShape(
                ShapeId.from("test#ConstrainedService"),
                ServiceShape::class.java,
            )
        service.errors.contains(validationExceptionShapeId).shouldBeFalse()
    }

    @Test
    fun `operations that do not have ValidationException will automatically have one added to them`() {
        verifyTestModelDoesNotHaveValidationException()

        serverIntegrationTest(
            testModelWithValidationExceptionImported,
            testCoverage = HttpTestType.Default,
        ) { codegenContext, _ ->
            // Verify the transformed model now has `smithy.framework#ValidationException` on the operation.
            val transformedOperation =
                codegenContext.model.expectShape(
                    ShapeId.from("test#SampleOperationWithoutValidation"),
                    OperationShape::class.java,
                )
            transformedOperation.errors.contains(validationExceptionShapeId).shouldBeTrue()
        }
    }

    private val testModelWithCustomValidationException =
        """
        namespace test

        use smithy.framework.rust#validationException
        use smithy.framework.rust#validationFieldList
        use aws.protocols#restJson1

        @restJson1
        service ConstrainedService {
            operations: [SampleOperation]
            errors: [CustomValidationException]
        }

        @http(uri: "/sample", method: "POST")
        operation SampleOperation {
            input: SampleInput
            output: SampleOutput
        }

        structure SampleInput {
            @range(min: 0, max: 100)
            constrainedInteger: Integer
        }

        structure SampleOutput {
            result: String
        }

        @error("client")
        @validationException
        structure CustomValidationException {
            message: String

            @validationFieldList
            fieldList: ValidationFieldList
        }

        structure ValidationField {
            name: String
        }

        list ValidationFieldList {
            member: ValidationField
        }
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `when custom validation exception exists, ValidationException is not automatically added`() {
        // Verify the operation doesn't have ValidationException in the original model
        val operation =
            testModelWithCustomValidationException.expectShape(
                ShapeId.from("test#SampleOperation"),
                OperationShape::class.java,
            )
        operation.errors.contains(validationExceptionShapeId).shouldBeFalse()

        // The model has a custom validation exception, so the transformer should NOT add
        // `smithy.framework#ValidationException`.
        serverIntegrationTest(
            testModelWithCustomValidationException,
            IntegrationTestParams(),
            testCoverage = HttpTestType.Default,
        ) { codegenContext, _ ->
            // Verify the transformed model still doesn't have `smithy.framework#ValidationException`
            // on the operation.
            val transformedOperation =
                codegenContext.model.expectShape(
                    ShapeId.from("test#SampleOperation"),
                    OperationShape::class.java,
                )
            transformedOperation.errors.contains(validationExceptionShapeId).shouldBeFalse()
        }
    }

    /**
     * Model with a constrained operation on a resource (not directly on the service), and without
     * `smithy.framework#ValidationException` in the model.
     */
    private val testModelWithResourceOperationAndNoValidationException =
        """
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service ConstrainedService {
            resources: [MyResource]
        }

        resource MyResource {
            operations: [ResourceOperation]
        }

        @http(uri: "/resource", method: "POST")
        operation ResourceOperation {
            input: ResourceInput
            output: ResourceOutput
        }

        structure ResourceInput {
            @range(min: 0, max: 100)
            constrainedInteger: Integer
        }

        structure ResourceOutput {}
        """.asSmithyModel(
            smithyVersion = "2",
            // Exclude smithy-validation-model from discovery so the shape is not in the model,
            // simulating the removeUnusedShapes projection transform pruning it.
            additionalDeniedModels = arrayOf("smithy-validation-model"),
        )

    @Test
    fun `resource operations get ValidationException even when the shape is not in the model`() {
        // Verify the shape is missing and the operation has no ValidationException.
        testModelWithResourceOperationAndNoValidationException
            .getShape(validationExceptionShapeId).isPresent.shouldBeFalse()
        testModelWithResourceOperationAndNoValidationException
            .expectShape(ShapeId.from("test#ResourceOperation"), OperationShape::class.java)
            .errors.contains(validationExceptionShapeId).shouldBeFalse()

        serverIntegrationTest(
            testModelWithResourceOperationAndNoValidationException,
            testCoverage = HttpTestType.Default,
        ) { codegenContext, _ ->
            // The shape should have been added and attached to the resource operation.
            codegenContext.model.getShape(validationExceptionShapeId).isPresent.shouldBeTrue()
            codegenContext.model
                .expectShape(ShapeId.from("test#ResourceOperation"), OperationShape::class.java)
                .errors.contains(validationExceptionShapeId).shouldBeTrue()
        }
    }
}
