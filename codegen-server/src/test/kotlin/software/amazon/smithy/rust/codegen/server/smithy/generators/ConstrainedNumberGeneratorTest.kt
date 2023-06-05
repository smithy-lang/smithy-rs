/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.createTestInlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import java.util.stream.Stream

class ConstrainedNumberGeneratorTest {

    data class TestCaseInputs(val constraintAnnotation: String, val validValue: Int, val invalidValue: Int)
    data class TestCase(val model: Model, val validValue: Int, val invalidValue: Int, val shapeName: String)

    class ConstrainedNumberGeneratorTestProvider : ArgumentsProvider {
        private val testCases = { type: String ->
            listOf(
                // Min and max.
                TestCaseInputs("@range(min: 10, max: 12)", 11, 13),
                // Min equal to max.
                TestCaseInputs("@range(min: 11, max: 11)", 11, 12),
                // Only min.
                TestCaseInputs("@range(min: 11)", 12, 2),
                // Only max.
                TestCaseInputs("@range(max: 11)", 0, 12),
            ).map {
                val shapeName = "Constrained${type.replaceFirstChar { c -> c.uppercaseChar() }}"
                TestCase(
                    """
                    namespace test

                    ${it.constraintAnnotation}
                    $type $shapeName
                    """.asSmithyModel(),
                    it.validValue,
                    it.invalidValue,
                    shapeName,
                )
            }
        }

        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
            listOf("integer", "short", "long", "byte").map { type -> testCases(type) }.flatten()
                .map { Arguments.of(it) }.stream()
    }

    @ParameterizedTest
    @ArgumentsSource(ConstrainedNumberGeneratorTestProvider::class)
    fun `it should generate constrained number types`(testCase: TestCase) {
        val shape = testCase.model.lookup<NumberShape>("test#${testCase.shapeName}")

        val codegenContext = serverTestCodegenContext(testCase.model)
        val symbolProvider = codegenContext.symbolProvider

        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(ServerRustModule.Model) {
            ConstrainedNumberGenerator(
                codegenContext,
                this.createTestInlineModuleCreator(),
                this,
                shape,
                SmithyValidationExceptionConversionGenerator(codegenContext),
            ).render()

            unitTest(
                name = "try_from_success",
                test = """
                    let _constrained: ${testCase.shapeName} = ${testCase.validValue}.try_into().unwrap();
                """,
            )
            unitTest(
                name = "try_from_fail",
                test = """
                    let constrained_res: Result<${testCase.shapeName}, _> = ${testCase.invalidValue}.try_into();
                    constrained_res.unwrap_err();
                """,
            )
            unitTest(
                name = "inner",
                test = """
                    let constrained = ${testCase.shapeName}::try_from(${testCase.validValue}).unwrap();
                    assert_eq!(constrained.inner(), &${testCase.validValue});
                """,
            )
            unitTest(
                name = "into_inner",
                test = """
                    let v = ${testCase.validValue};
                    let constrained = ${testCase.shapeName}::try_from(v).unwrap();

                    assert_eq!(constrained.into_inner(), v);
                """,
            )
        }

        project.compileAndTest()
    }

    class NoStructuralConstructorTestProvider : ArgumentsProvider {
        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
            listOf(
                Triple("byte", "ConstrainedByte", "i8"),
                Triple("short", "ConstrainedShort", "i16"),
                Triple("integer", "ConstrainedInteger", "i32"),
                Triple("long", "ConstrainedLong", "i64"),
            ).map { Arguments.of(it) }.stream()
    }

    @ParameterizedTest
    @ArgumentsSource(NoStructuralConstructorTestProvider::class)
    fun `type should not be constructable without using a constructor`(args: Triple<String, String, String>) {
        val (smithyType, shapeName, rustType) = args
        val model = """
            namespace test

            @range(min: -1, max: 5)
            $smithyType $shapeName
        """.asSmithyModel()
        val constrainedShape = model.lookup<NumberShape>("test#$shapeName")

        val codegenContext = serverTestCodegenContext(model)

        val writer = RustWriter.forModule(ServerRustModule.Model.name)
        ConstrainedNumberGenerator(
            codegenContext,
            writer.createTestInlineModuleCreator(),
            writer,
            constrainedShape,
            SmithyValidationExceptionConversionGenerator(codegenContext),
        ).render()

        // Check that the wrapped type is `pub(crate)`.
        writer.toString() shouldContain "pub struct $shapeName(pub(crate) $rustType);"
    }
}
