/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.transformers

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.testutil.asSmithy
import software.amazon.smithy.rust.testutil.testSymbolProvider

internal class OperationNormalizerTest {

    @Test
    fun `add inputs to empty operations`() {
        val model = """
            namespace smithy.test
            operation Empty {}
        """.asSmithy()
        val operationId = ShapeId.from("smithy.test#Empty")
        model.expectShape(operationId, OperationShape::class.java).input.isPresent shouldBe false
        val sut = OperationNormalizer(testSymbolProvider(model))
        val modified = sut.addOperationInputs(model)
        val operation = modified.expectShape(operationId, OperationShape::class.java)
        operation.input.isPresent shouldBe true
        operation.input.get().name shouldBe "EmptyInput"
    }

    @Test
    fun `create cloned inputs for operations`() {
        val model = """
            namespace smithy.test
            structure RenameMe {
                v: String
            }
            operation MyOp {
                input: RenameMe
            }
        """.asSmithy()
        val operationId = ShapeId.from("smithy.test#MyOp")
        model.expectShape(operationId, OperationShape::class.java).input.isPresent shouldBe true
        val sut = OperationNormalizer(testSymbolProvider(model))
        val modified = sut.addOperationInputs(model)
        val operation = modified.expectShape(operationId, OperationShape::class.java)
        operation.input.isPresent shouldBe true
        val inputId = operation.input.get()
        inputId.name shouldBe "MyOpInput"
        val inputShape = modified.expectShape(inputId, StructureShape::class.java)
        testSymbolProvider(modified).toSymbol(inputShape).name shouldBe "MyOpInput"
        inputShape.memberNames shouldBe listOf("v")
    }
}
