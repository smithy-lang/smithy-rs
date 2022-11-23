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
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import java.util.stream.Stream

class ConstrainedIntegerGeneratorTest {

    data class TestCase(val model: Model, val validInteger: Int, val invalidInteger: Int)

    class ConstrainedIntGeneratorTestProvider : ArgumentsProvider {
        private val testCases = listOf(
            // Min and max.
            Triple("@range(min: 10, max: 12)", 11, 13),
            // Min equal to max.
            Triple("@range(min: 11, max: 11)", 11, 12),
            // Only min.
            Triple("@range(min: 11)", 12, 2),
            // Only max.
            Triple("@range(max: 11)", 0, 12),
        ).map {
            TestCase(
                """
                namespace test

                ${it.first}
                integer ConstrainedInteger
                """.asSmithyModel(),
                it.second,
                it.third,
            )
        }

        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
            testCases.map { Arguments.of(it) }.stream()
    }

    @ParameterizedTest
    @ArgumentsSource(ConstrainedIntGeneratorTestProvider::class)
    fun `it should generate constrained integer types`(testCase: TestCase) {
        val constrainedIntegerShape = testCase.model.lookup<IntegerShape>("test#ConstrainedInteger")

        val codegenContext = serverTestCodegenContext(testCase.model)
        val symbolProvider = codegenContext.symbolProvider

        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(ModelsModule) {
            ConstrainedIntegerGenerator(codegenContext, this, constrainedIntegerShape).render()

            unitTest(
                name = "try_from_success",
                test = """
                    let _constrained: ConstrainedInteger = ${testCase.validInteger}.try_into().unwrap();
                """,
            )
            unitTest(
                name = "try_from_fail",
                test = """
                    let constrained_res: Result<ConstrainedInteger, _> = ${testCase.invalidInteger}.try_into();
                    constrained_res.unwrap_err();
                """,
            )
            unitTest(
                name = "inner",
                test = """
                    let constrained = ConstrainedInteger::try_from(${testCase.validInteger}).unwrap();
                    assert_eq!(constrained.inner(), &${testCase.validInteger});
                """,
            )
            unitTest(
                name = "into_inner",
                test = """
                    let int = ${testCase.validInteger};
                    let constrained = ConstrainedInteger::try_from(int).unwrap();

                    assert_eq!(constrained.into_inner(), int);
                """,
            )
        }

        project.compileAndTest()
    }

    @Test
    fun `type should not be constructible without using a constructor`() {
        val model = """
            namespace test

            @range(min: -1, max: 69)
            integer ConstrainedInteger
        """.asSmithyModel()
        val constrainedIntegerShape = model.lookup<IntegerShape>("test#ConstrainedInteger")

        val codegenContext = serverTestCodegenContext(model)

        val writer = RustWriter.forModule(ModelsModule.name)

        ConstrainedIntegerGenerator(codegenContext, writer, constrainedIntegerShape).render()

        // Check that the wrapped type is `pub(crate)`.
        writer.toString() shouldContain "pub struct ConstrainedInteger(pub(crate) i32);"
    }
}
