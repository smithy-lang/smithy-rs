/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Tests whether the server `codegen` flag `addValidationExceptionToConstrainedOperations` works as expected.
 */
internal class AddValidationExceptionToConstrainedOperationsTest {
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

    @Test
    fun `without setting the codegen flag, the model should fail to compile`() {
        assertThrows<CodegenException> {
            serverIntegrationTest(
                testModelWithValidationExceptionImported,
                IntegrationTestParams(),
                testCoverage = HttpTestType.AsConfigured,
            )
        }
    }

    @Test
    fun `operations that do not have ValidationException will automatically have one added to them`() {
        serverIntegrationTest(
            testModelWithValidationExceptionImported,
            IntegrationTestParams(
                additionalSettings =
                    ServerAdditionalSettings.builder()
                        .addValidationExceptionToConstrainedOperations()
                        .toObjectNode(),
            ),
            testCoverage = HttpTestType.AsConfigured,
        )
    }
}
