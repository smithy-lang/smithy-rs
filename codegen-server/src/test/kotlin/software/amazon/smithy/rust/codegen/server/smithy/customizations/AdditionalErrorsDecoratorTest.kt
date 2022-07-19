/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.util.lookup

class AdditionalErrorsDecoratorTest {
    private val baseModel = """
        namespace test

        operation Infallible {
            input: InputOutput,
            output: InputOutput
        }

        operation Fallible {
            input: InputOutput,
            output: InputOutput,
            errors: [AnError]
        }

        structure InputOutput { }

        @error("client")
        structure AnError { }
    """.asSmithyModel()
    private val model = OperationNormalizer.transform(baseModel)
    private val service = ServiceShape.builder().id("smithy.test#Test").build()

    @Test
    fun `add InternalServerError to infallible operations only`() {
        model.lookup<OperationShape>("test#Infallible").errors.isEmpty() shouldBe true
        model.lookup<OperationShape>("test#Fallible").errors.size shouldBe 1
        val transformedModel = AddInternalServerErrorToInfallibleOperationsDecorator().transformModel(service, model)
        transformedModel.lookup<OperationShape>("test#Infallible").errors.size shouldBe 1
        transformedModel.lookup<OperationShape>("test#Fallible").errors.size shouldBe 1
    }

    @Test
    fun `add InternalServerError to all model operations`() {
        model.lookup<OperationShape>("test#Infallible").errors.isEmpty() shouldBe true
        model.lookup<OperationShape>("test#Fallible").errors.size shouldBe 1
        val transformedModel = AddInternalServerErrorToAllOperationsDecorator().transformModel(service, model)
        transformedModel.lookup<OperationShape>("test#Infallible").errors.size shouldBe 1
        transformedModel.lookup<OperationShape>("test#Fallible").errors.size shouldBe 2
    }
}
