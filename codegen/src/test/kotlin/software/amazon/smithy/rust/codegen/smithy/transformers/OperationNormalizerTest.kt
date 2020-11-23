/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.transformers

import io.kotest.matchers.shouldBe
import io.kotest.matchers.shouldNotBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.smithy.traits.InputBodyTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.testutil.asSmithy
import software.amazon.smithy.rust.testutil.testSymbolProvider

internal class OperationNormalizerTest {

    @Test
    fun `add inputs and outputs to empty operations`() {
        val model = """
            namespace smithy.test
            operation Empty {}
        """.asSmithy()
        val operationId = ShapeId.from("smithy.test#Empty")
        model.expectShape(operationId, OperationShape::class.java).input.isPresent shouldBe false
        val sut = OperationNormalizer()
        val modified = sut.transformModel(model, OperationNormalizer.noBody, OperationNormalizer.noBody)
        val operation = modified.expectShape(operationId, OperationShape::class.java)
        operation.input.isPresent shouldBe true
        operation.input.get().name shouldBe "EmptyInput"

        val inputShape = modified.expectShape(operation.input.get(), StructureShape::class.java)
        val inputTrait = inputShape.expectTrait(SyntheticInputTrait::class.java)
        // When there isn't an input, we shouldn't attach a body
        inputTrait.body shouldBe null

        operation.output.orNull() shouldNotBe null
        operation.output.get().name shouldBe "EmptyOutput"

        val outputShape = modified.expectShape(operation.output.get(), StructureShape::class.java)
        val outputTrait = outputShape.expectTrait(SyntheticOutputTrait::class.java)
        // When there isn't an output, we shouldn't attach a body
        outputTrait.body shouldBe null
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
        val sut = OperationNormalizer()
        val modified = sut.transformModel(model, OperationNormalizer.noBody, OperationNormalizer.noBody)
        val operation = modified.expectShape(operationId, OperationShape::class.java)
        operation.input.isPresent shouldBe true
        val inputId = operation.input.get()
        inputId.name shouldBe "MyOpInput"
        val inputShape = modified.expectShape(inputId, StructureShape::class.java)
        testSymbolProvider(modified).toSymbol(inputShape).name shouldBe "MyOpInput"
        inputShape.memberNames shouldBe listOf("v")
    }

    @Test
    fun `create bodies for operations`() {
        val model = """
            namespace smithy.test
            structure RenameMe {
                v: String,
                drop: String
            }
            operation MyOp {
                input: RenameMe
            }""".asSmithy()

        val sut = OperationNormalizer()
        val modified = sut.transformModel(
            model,
            inputBody = { input ->
                input?.toBuilder()?.members(input.members().filter { it.memberName != "drop" })?.build()
            },
            outputBody = OperationNormalizer.noBody
        )
        val operation = modified.lookup<OperationShape>("smithy.test#MyOp")
        operation.input.isPresent shouldBe true
        val inputId = operation.input.get()
        inputId.name shouldBe "MyOpInput"
        val inputShape = modified.expectShape(inputId, StructureShape::class.java)
        val input = inputShape.expectTrait(SyntheticInputTrait::class.java)
        input.body shouldBe ShapeId.from("smithy.test#MyOpInputBody")
        val body = modified.expectShape(input.body, StructureShape::class.java)
        body.expectTrait(InputBodyTrait::class.java)
        body.members().size shouldBe 1
    }
}
