/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
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
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.client.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.client.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.client.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.client.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.client.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.client.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import java.util.stream.Stream

private const val baseModelString = """
    namespace test

    service TestService {
        version: "123",
        operations: [TestOperation]
    }
    
    operation TestOperation {
        input: TestInputOutput,
        output: TestInputOutput,
    }
    
    structure TestInputOutput {
        constrainedString: ConstrainedString
    }
    """

class ConstrainedStringGeneratorTest {

    data class TestCase(val model: Model, val validString: String, val invalidString: String)

    class ConstrainedStringGeneratorTestProvider : ArgumentsProvider {
        private val testCases = listOf(
            // Min and max.
            Triple("@length(min: 11, max: 12)", "validString", "invalidString"),
            // Min equal to max.
            Triple("@length(min: 11, max: 11)", "validString", "invalidString"),
            // Only min.
            Triple("@length(min: 11)", "validString", ""),
            // Only max.
            Triple("@length(max: 11)", "", "invalidString"),
            // Count Unicode scalar values, not `.len()`.
            Triple(
                "@length(min: 3, max: 3)",
                "👍👍👍", // These three emojis are three Unicode scalar values.
                "👍👍👍👍",
            ),
        ).map {
            TestCase(
                """
                $baseModelString
                
                ${it.first}
                string ConstrainedString
                """.asSmithyModel(),
                it.second,
                it.third,
            )
        }

        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
            testCases.map { Arguments.of(it) }.stream()
    }

    @ParameterizedTest
    @ArgumentsSource(ConstrainedStringGeneratorTestProvider::class)
    fun `it should generate constrained string types`(testCase: TestCase) {
        val constrainedStringShape = testCase.model.lookup<StringShape>("test#ConstrainedString")

        val codegenContext = serverTestCodegenContext(testCase.model)
        val symbolProvider = codegenContext.symbolProvider

        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(ModelsModule) { writer ->
            ConstrainedStringGenerator(codegenContext, writer, constrainedStringShape).render()

            writer.unitTest(
                name = "parse_success",
                test = """
                    let string = String::from("${testCase.validString}");
                    let _constrained = ConstrainedString::parse(string).unwrap();
                """,
            )
            writer.unitTest(
                name = "try_from_success",
                test = """
                    let string = String::from("${testCase.validString}");
                    let _constrained: ConstrainedString = string.try_into().unwrap();
                """,
            )
            writer.unitTest(
                name = "parse_fail",
                test = """
                    let string = String::from("${testCase.invalidString}");
                    let _constrained = ConstrainedString::parse(string).unwrap_err();
                """,
            )
            writer.unitTest(
                name = "try_from_fail",
                test = """
                    let string = String::from("${testCase.invalidString}");
                    let constrained_res: Result<ConstrainedString, _> = string.try_into();
                    constrained_res.unwrap_err();
                """,
            )
            writer.unitTest(
                name = "inner",
                test = """
                    let string = String::from("${testCase.validString}");
                    let constrained = ConstrainedString::parse(string).unwrap();

                    assert_eq!(constrained.inner(), "${testCase.validString}");
                """,
            )
            writer.unitTest(
                name = "into_inner",
                test = """
                    let string = String::from("${testCase.validString}");
                    let constrained = ConstrainedString::parse(string.clone()).unwrap();

                    assert_eq!(constrained.into_inner(), string);
                """,
            )
        }

        project.compileAndTest()
    }

    @Test
    fun `type should not be constructible without using a constructor`() {
        val model = """
            $baseModelString
            
            @length(min: 1, max: 69)
            string ConstrainedString
            """.asSmithyModel()
        val constrainedStringShape = model.lookup<StringShape>("test#ConstrainedString")

        val codegenContext = serverTestCodegenContext(model)

        val writer = RustWriter.forModule(ModelsModule.name)

        ConstrainedStringGenerator(codegenContext, writer, constrainedStringShape).render()

        // Check that the wrapped type is `pub(crate)`.
        writer.toString() shouldContain "pub struct ConstrainedString(pub(crate) std::string::String);"
    }
}
