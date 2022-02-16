/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel

internal class FallibleOperationsDecoratorTest {
    @Test
    fun `add inputs and outputs to empty operations`() {
        val model = """
            namespace smithy.test
            operation Empty {}
        """.asSmithyModel()
        val operationId = ShapeId.from("smithy.test#Empty")
        model.expectShape(operationId, OperationShape::class.java).input.isPresent shouldBe false
    }
}
