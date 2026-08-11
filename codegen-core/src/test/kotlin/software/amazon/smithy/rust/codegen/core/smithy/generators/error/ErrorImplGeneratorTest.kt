/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.error

import io.kotest.matchers.string.shouldContain
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
                assert_eq!(err.to_string(), "MyError");

                let err = MyError::builder().message("message").build();
                assert_eq!(err.to_string(), "MyError: message");
                """,
            )
        }
    }

    @Test
    fun `required error messages format without redundant borrows`() {
        val requiredMessageModel =
            """
            namespace com.test

            @error("server")
            structure RequiredMessageError {
                @required
                message: String
            }
            """.asSmithyModel()
        val provider = testSymbolProvider(requiredMessageModel)
        val project = TestWorkspace.testProject(provider)
        val errorShape = requiredMessageModel.expectShape(ShapeId.from("com.test#RequiredMessageError")) as StructureShape
        errorShape.renderWithModelBuilder(requiredMessageModel, provider, project)
        project.moduleFor(errorShape) {
            val errorTrait = errorShape.getTrait<ErrorTrait>()!!
            ErrorImplGenerator(requiredMessageModel, provider, this, errorShape, errorTrait, emptyList()).render(CodegenTarget.CLIENT)

            toString() shouldContain """::std::write!(f, ": {}", self.message)?;"""
            compileAndTest(
                """
                let err = RequiredMessageError::builder().message("message").build().unwrap();
                assert_eq!(err.to_string(), "RequiredMessageError: message");
                """,
            )
        }
    }
}
