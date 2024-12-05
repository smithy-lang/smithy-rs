/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.model.Model
import software.amazon.smithy.rulesengine.traits.EndpointTestOperationInput
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rustsdk.endpoints.operationId

class OperationInputTestGeneratorTests {
    @Test
    fun `finds operation shape by name`() {
        val prefix = "\$version: \"2\""
        val operationModel =
            """
            $prefix
            namespace operations

            operation Ping {}
            """.trimIndent()
        val serviceModel =
            """
            $prefix
            namespace service

            use operations#Ping

            service MyService {
                operations: [Ping]
            }
            """.trimIndent()

        val model =
            Model.assembler()
                .discoverModels()
                .addUnparsedModel("operation.smithy", operationModel)
                .addUnparsedModel("main.smithy", serviceModel)
                .assemble()
                .unwrap()

        val context = testClientCodegenContext(model)
        val testOperationInput =
            EndpointTestOperationInput.builder()
                .operationName("Ping")
                .build()

        val operationId = context.operationId(testOperationInput)
        assertEquals("operations#Ping", operationId.toString())
    }

    @Test
    fun `finds operation shape by name from nested operations`() {
        val prefix = "\$version: \"2\""
        val operationModel =
            """
            $prefix
            namespace operations.bells

            resource Bell {
                operations: [Ding]
            }

            operation Ding {}
            """.trimIndent()
        val serviceModel =
            """
            $prefix
            namespace service

            use operations.bells#Bell

            service MyService {
                resources: [Bell]
            }
            """.trimIndent()

        val model =
            Model.assembler()
                .discoverModels()
                .addUnparsedModel("operation.smithy", operationModel)
                .addUnparsedModel("main.smithy", serviceModel)
                .assemble()
                .unwrap()

        val context = testClientCodegenContext(model)
        val testOperationInput =
            EndpointTestOperationInput.builder()
                .operationName("Ding")
                .build()

        val operationId = context.operationId(testOperationInput)
        assertEquals("operations.bells#Ding", operationId.toString())
    }

    @Test
    fun `fails for operation name not found`() {
        val model =
            """
            namespace test
            operation Ping {}
            service MyService {
                operations: [Ping]
            }
            """.trimIndent().asSmithyModel()

        val context = testClientCodegenContext(model)
        val testOperationInput =
            EndpointTestOperationInput.builder()
                .operationName("Pong")
                .build()

        assertThrows<NoSuchElementException> { context.operationId(testOperationInput) }
    }
}
