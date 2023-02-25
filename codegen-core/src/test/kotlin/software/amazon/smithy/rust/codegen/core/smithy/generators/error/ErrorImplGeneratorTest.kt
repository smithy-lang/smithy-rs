/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.error

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.getTrait

class ErrorImplGeneratorTest {
    val model =
        """
        namespace com.test

        @error("server")
        @retryable
        structure MyError {
            message: String
        }
        """.asSmithyModel()

    @Test
    fun `generate error structures`() {
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        val errorShape = model.expectShape(ShapeId.from("com.test#MyError")) as StructureShape
        errorShape.renderWithModelBuilder(model, provider, project)
        project.moduleFor(errorShape) {
            val errorTrait = errorShape.getTrait<ErrorTrait>()!!
            ErrorImplGenerator(model, provider, this, errorShape, errorTrait, emptyList()).render(CodegenTarget.CLIENT)
            compileAndTest(
                """
                let err = MyError::builder().build();
                assert_eq!(err.retryable_error_kind(), aws_smithy_types::retry::ErrorKind::ServerError);
                """,
            )
        }
    }
}
