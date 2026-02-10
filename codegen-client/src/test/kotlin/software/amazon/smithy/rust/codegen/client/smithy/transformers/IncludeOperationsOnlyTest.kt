/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.transformers

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class IncludeOperationsOnlyTest {
    private val baseModel =
        """
        namespace test

        service TestService {
            operations: [OperationA, OperationB, OperationC]
        }

        operation OperationA {}
        operation OperationB {}
        operation OperationC {}
        """.asSmithyModel()

    @Test
    fun `returns original model when includeOperations is empty`() {
        val result = IncludeOperationsOnly.transformModel(baseModel, emptySet())
        result shouldBe baseModel
    }

    @Test
    fun `filters operations correctly`() {
        val result = IncludeOperationsOnly.transformModel(baseModel, setOf("OperationA", "OperationC"))
        val service = result.expectShape(ShapeId.from("test#TestService"), ServiceShape::class.java)

        service.allOperations.map { it.name }.toSet() shouldBe setOf("OperationA", "OperationC")
    }

    @Test
    fun `throws exception for non-existent operation`() {
        val exception =
            org.junit.jupiter.api.assertThrows<IllegalArgumentException> {
                IncludeOperationsOnly.transformModel(baseModel, setOf("NonExistent"))
            }

        exception.message shouldBe "Operations not found in service test#TestService: NonExistent\n" +
            "Available operations: OperationA, OperationB, OperationC"
    }

    @Test
    fun `throws exception for partially invalid operations`() {
        val exception =
            org.junit.jupiter.api.assertThrows<IllegalArgumentException> {
                IncludeOperationsOnly.transformModel(baseModel, setOf("OperationA", "Invalid1", "Invalid2"))
            }

        exception.message shouldBe "Operations not found in service test#TestService: Invalid1, Invalid2\n" +
            "Available operations: OperationA, OperationB, OperationC"
    }
}
