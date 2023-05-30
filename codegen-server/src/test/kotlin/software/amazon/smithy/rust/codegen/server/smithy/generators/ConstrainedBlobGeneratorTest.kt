/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.createTestInlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import java.util.stream.Stream

class ConstrainedBlobGeneratorTest {
    data class TestCase(val model: Model, val validBlob: String, val invalidBlob: String)

    class ConstrainedBlobGeneratorTestProvider : ArgumentsProvider {
        private val testCases = listOf(
            // Min and max.
            Triple("@length(min: 11, max: 12)", "validString", "invalidString"),
            // Min equal to max.
            Triple("@length(min: 11, max: 11)", "validString", "invalidString"),
            // Only min.
            Triple("@length(min: 11)", "validString", ""),
            // Only max.
            Triple("@length(max: 11)", "", "invalidString"),
        ).map {
            TestCase(
                """
                namespace test

                ${it.first}
                blob ConstrainedBlob
                """.asSmithyModel(),
                "aws_smithy_types::Blob::new(Vec::from(${it.second.dq()}))",
                "aws_smithy_types::Blob::new(Vec::from(${it.third.dq()}))",
            )
        }

        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
            testCases.map { Arguments.of(it) }.stream()
    }

    @ParameterizedTest
    @ArgumentsSource(ConstrainedBlobGeneratorTestProvider::class)
    fun `it should generate constrained blob types`(testCase: TestCase) {
        val constrainedBlobShape = testCase.model.lookup<BlobShape>("test#ConstrainedBlob")

        val codegenContext = serverTestCodegenContext(testCase.model)
        val symbolProvider = codegenContext.symbolProvider

        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(ServerRustModule.Model) {
            addDependency(RuntimeType.blob(codegenContext.runtimeConfig).toSymbol())

            ConstrainedBlobGenerator(
                codegenContext,
                this.createTestInlineModuleCreator(),
                this,
                constrainedBlobShape,
                SmithyValidationExceptionConversionGenerator(codegenContext),
            ).render()

            unitTest(
                name = "try_from_success",
                test = """
                    let blob = ${testCase.validBlob};
                    let _constrained = ConstrainedBlob::try_from(blob).unwrap();
                """,
            )
            unitTest(
                name = "try_from_fail",
                test = """
                    let blob = ${testCase.invalidBlob};
                    ConstrainedBlob::try_from(blob).unwrap_err();
                """,
            )
            unitTest(
                name = "inner",
                test = """
                    let blob = ${testCase.validBlob};
                    let constrained = ConstrainedBlob::try_from(blob).unwrap();

                    assert_eq!(*constrained.inner(), ${testCase.validBlob});
                """,
            )
            unitTest(
                name = "into_inner",
                test = """
                    let blob = ${testCase.validBlob};
                    let constrained = ConstrainedBlob::try_from(blob).unwrap();

                    assert_eq!(constrained.into_inner(), ${testCase.validBlob});
                """,
            )
        }

        project.compileAndTest()
    }

    @Test
    fun `type should not be constructable without using a constructor`() {
        val model = """
            namespace test

            @length(min: 1, max: 70)
            blob ConstrainedBlob
        """.asSmithyModel()
        val constrainedBlobShape = model.lookup<BlobShape>("test#ConstrainedBlob")

        val codegenContext = serverTestCodegenContext(model)

        val writer = RustWriter.forModule(ServerRustModule.Model.name)

        ConstrainedBlobGenerator(
            codegenContext,
            writer.createTestInlineModuleCreator(),
            writer,
            constrainedBlobShape,
            SmithyValidationExceptionConversionGenerator(codegenContext),
        ).render()

        // Check that the wrapped type is `pub(crate)`.
        writer.toString() shouldContain "pub struct ConstrainedBlob(pub(crate) ::aws_smithy_types::Blob);"
    }
}
