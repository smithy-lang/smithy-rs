/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel

class AdditionalErrorsDecoratorTest {
    private val baseModel = """
        namespace smithy.test
        use aws.protocols#restJson1

        @restJson1
        @title("Test")
        service Test {
            version: "2022-01-01",
            operations: [
                Infallible,
                Fallible
            ]
        }

        operation Infallible {
            input: InfallibleInput,
            output: InfallibleOutput
        }
        structure InfallibleInput {}
        structure InfallibleOutput {}

        operation Fallible {
            input: FallibleInput,
            output: FallibleOutput,
            errors: [FallibleError]
        }
        structure FallibleInput {}
        structure FallibleOutput {}
        @error("client")
        structure FallibleError {}
    """.asSmithyModel()
    private val model = OperationNormalizer.transform(baseModel)
    private val symbolProvider = serverTestSymbolProvider(model)

    @Test
    fun `add InternalServerError to infallible operations only`() {
        val service = ServiceShape.builder().id("smithy.test#Test").build()
        val infallibleId = ShapeId.from("smithy.test#Infallible")
        val fallibleId = ShapeId.from("smithy.test#Fallible")
        model.expectShape(infallibleId, OperationShape::class.java).errors.isEmpty() shouldBe true
        model.expectShape(fallibleId, OperationShape::class.java).errors.size shouldBe 1
        val model = FallibleOperationsDecorator().transformModel(service, model)
        model.expectShape(infallibleId, OperationShape::class.java).errors.isEmpty() shouldBe false
        model.expectShape(infallibleId, OperationShape::class.java).errors.size shouldBe 1
        model.expectShape(fallibleId, OperationShape::class.java).errors.size shouldBe 1
    }

    @Test
    fun `add InternalServerError to all model operations`() {
        val service = ServiceShape.builder().id("smithy.test#Test").build()
        val infallibleId = ShapeId.from("smithy.test#Infallible")
        val fallibleId = ShapeId.from("smithy.test#Fallible")
        model.expectShape(infallibleId, OperationShape::class.java).errors.isEmpty() shouldBe true
        model.expectShape(fallibleId, OperationShape::class.java).errors.size shouldBe 1
        val model = InternalServerErrorDecorator().transformModel(service, model)
        model.expectShape(infallibleId, OperationShape::class.java).errors.isEmpty() shouldBe false
        model.expectShape(infallibleId, OperationShape::class.java).errors.size shouldBe 1
        model.expectShape(fallibleId, OperationShape::class.java).errors.size shouldBe 2
    }
}
