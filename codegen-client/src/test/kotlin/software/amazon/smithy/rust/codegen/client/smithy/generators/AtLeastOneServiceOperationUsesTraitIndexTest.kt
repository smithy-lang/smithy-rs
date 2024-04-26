/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup

class AtLeastOneServiceOperationUsesTraitIndexTest {
    val model =
        """
        namespace com.example
        service TestService {
            operations: [

            ]
        }

        @sensitive
        operation A {
            output := { }
        }

        @requestCompression
        operation B {
            input: {  }
        }

        structure OpBInput {
            @httpPayload
            body: String
        }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `correctly tracks operation trait use`() {
        val index = SensitiveIndex.of(model)

        data class TestCase(val shape: String, val sensitiveInput: Boolean, val sensitiveOutput: Boolean)

        val testCases =
            listOf(
                TestCase("NotSensitive", sensitiveInput = false, sensitiveOutput = false),
                TestCase("SensitiveInput", sensitiveInput = true, sensitiveOutput = false),
                TestCase("SensitiveOutput", sensitiveInput = false, sensitiveOutput = true),
                TestCase("NestedSensitiveInput", sensitiveInput = true, sensitiveOutput = false),
                TestCase("NestedSensitiveOutput", sensitiveInput = false, sensitiveOutput = true),
            )
        testCases.forEach { tc ->
            assertEquals(tc.sensitiveInput, index.hasSensitiveInput(model.lookup("com.example#${tc.shape}")), "input: $tc")
            assertEquals(tc.sensitiveOutput, index.hasSensitiveOutput(model.lookup("com.example#${tc.shape}")), "output: $tc ")
        }
    }
}
