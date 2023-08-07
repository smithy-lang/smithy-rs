/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup

class SensitiveIndexTest {
    val model = """
        namespace com.example
        service TestService {
            operations: [
                NotSensitive,
                SensitiveInput,
                SensitiveOutput,
                NestedSensitiveInput,
                NestedSensitiveOutput
            ]
        }

        @sensitive
        structure Credentials {
           username: String,
           password: String
        }

        operation NotSensitive {
            input := { userId: String }

            output := { response: String }
        }

        operation SensitiveInput {
            input := { credentials: Credentials }
        }

        operation SensitiveOutput {
            output := { credentials: Credentials }
        }

        operation NestedSensitiveInput {
            input := { nested: Nested }
        }

        operation NestedSensitiveOutput {
            output := { nested: Nested }
        }

        structure Nested {
            inner: Inner
        }

        structure Inner {
            credentials: Credentials
        }
    """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `correctly identify operations`() {
        val index = SensitiveIndex.of(model)

        data class TestCase(val shape: String, val sensitiveInput: Boolean, val sensitiveOutput: Boolean)

        val testCases = listOf(
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
