/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.transformers

import io.kotest.matchers.shouldBe
import io.kotest.matchers.shouldNotBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.util.orNull

internal class OperationNormalizerTest {
    @Test
    fun `add inputs and outputs to empty operations`() {
        val model = """
            namespace smithy.test
            operation Empty {}
        """.asSmithyModel()
        val operationId = ShapeId.from("smithy.test#Empty")
        model.expectShape(operationId, OperationShape::class.java).input.isPresent shouldBe false
        val modified = OperationNormalizer.transform(model)
        val operation = modified.expectShape(operationId, OperationShape::class.java)
        operation.input.isPresent shouldBe true
        operation.input.get().name shouldBe "EmptyInput"

        val inputShape = modified.expectShape(operation.input.get(), StructureShape::class.java)
        inputShape.expectTrait(SyntheticInputTrait::class.java)

        operation.output.orNull() shouldNotBe null
        operation.output.get().name shouldBe "EmptyOutput"

        val outputShape = modified.expectShape(operation.output.get(), StructureShape::class.java)
        outputShape.expectTrait(SyntheticOutputTrait::class.java)
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
        """.asSmithyModel()
        val operationId = ShapeId.from("smithy.test#MyOp")
        model.expectShape(operationId, OperationShape::class.java).input.isPresent shouldBe true
        val modified = OperationNormalizer.transform(model)
        val operation = modified.expectShape(operationId, OperationShape::class.java)
        operation.input.isPresent shouldBe true
        val inputId = operation.input.get()
        inputId.name shouldBe "MyOpInput"
        val inputShape = modified.expectShape(inputId, StructureShape::class.java)
        testSymbolProvider(modified).toSymbol(inputShape).name shouldBe "MyOpInput"
        inputShape.memberNames shouldBe listOf("v")
    }

    @Test
    fun `create cloned outputs for operations`() {
        val model = """
            namespace smithy.test
            structure RenameMe {
                v: String
            }
            operation MyOp {
                output: RenameMe
            }
        """.asSmithyModel()
        val operationId = ShapeId.from("smithy.test#MyOp")
        model.expectShape(operationId, OperationShape::class.java).output.isPresent shouldBe true
        val modified = OperationNormalizer.transform(model)
        val operation = modified.expectShape(operationId, OperationShape::class.java)
        operation.output.isPresent shouldBe true
        val outputId = operation.output.get()
        outputId.name shouldBe "MyOpOutput"
        val outputShape = modified.expectShape(outputId, StructureShape::class.java)
        testSymbolProvider(modified).toSymbol(outputShape).name shouldBe "MyOpOutput"
        outputShape.memberNames shouldBe listOf("v")
    }

    @Test
    fun `synthetics should not collide with other operations`() {
        val model = """
            namespace test

            structure DeleteApplicationRequest {}
            structure DeleteApplicationResponse {}

            operation DeleteApplication {
                input: DeleteApplicationRequest,
                output: DeleteApplicationResponse,
            }

            structure DeleteApplicationOutputRequest {}
            structure DeleteApplicationOutputResponse {}

            operation DeleteApplicationOutput {
                input: DeleteApplicationOutputRequest,
                output: DeleteApplicationOutputResponse,
            }
        """.asSmithyModel()

        (model.expectShape(ShapeId.from("test#DeleteApplicationOutput")) is OperationShape) shouldBe true

        val modified = OperationNormalizer.transform(model)
        (modified.expectShape(ShapeId.from("test#DeleteApplicationOutput")) is OperationShape) shouldBe true
    }
}
