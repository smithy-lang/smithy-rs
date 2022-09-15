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
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.client.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.client.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.client.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.client.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.client.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.client.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.client.testutil.unitTest
import software.amazon.smithy.rust.codegen.client.util.lookup
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
        constrainedMap: ConstrainedMap
    }
    """

class ConstrainedMapGeneratorTest {

    data class TestCase(val model: Model, val validMap: ObjectNode, val invalidMap: ObjectNode)

    class ConstrainedMapGeneratorTestProvider : ArgumentsProvider {
        private val testCases = listOf(
            // Min and max.
            Triple("@length(min: 11, max: 12)", 11, 13),
            // Min equal to max.
            Triple("@length(min: 11, max: 11)", 11, 12),
            // Only min.
            Triple("@length(min: 11)", 15, 10),
            // Only max.
            Triple("@length(max: 11)", 11, 12),
        ).map {
            val validStringMap = List(it.second) { index -> index.toString() to "value" }.toMap()
            val inValidStringMap = List(it.third) { index -> index.toString() to "value" }.toMap()
            Triple(it.first, ObjectNode.fromStringMap(validStringMap), ObjectNode.fromStringMap(inValidStringMap))
        }.map {
            TestCase(
                """
                $baseModelString
                
                ${it.first}
                map ConstrainedMap {
                    key: String,
                    value: String
                }
                """.asSmithyModel(),
                it.second,
                it.third,
            )
        }

        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
            testCases.map { Arguments.of(it) }.stream()
    }

    @ParameterizedTest
    @ArgumentsSource(ConstrainedMapGeneratorTestProvider::class)
    fun `it should generate constrained map types`(testCase: TestCase) {
        val serviceShape = testCase.model.lookup<ServiceShape>("test#TestService")
        val constrainedMapShape = testCase.model.lookup<MapShape>("test#ConstrainedMap")

        val codegenContext = serverTestCodegenContext(testCase.model, serviceShape)
        val symbolProvider = codegenContext.symbolProvider

        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(ModelsModule) { writer ->
            render(codegenContext, writer, constrainedMapShape)

            val instantiator =
                Instantiator(symbolProvider, testCase.model, codegenContext.runtimeConfig, codegenContext.target)
            writer.rustBlock("##[cfg(test)] fn build_valid_map() -> std::collections::HashMap<String, String>") {
                instantiator.render(this, constrainedMapShape, testCase.validMap)
            }
            writer.rustBlock("##[cfg(test)] fn build_invalid_map() -> std::collections::HashMap<String, String>") {
                instantiator.render(this, constrainedMapShape, testCase.invalidMap)
            }

            writer.unitTest(
                name = "parse_success",
                test = """
                    let map = build_valid_map();
                    let _constrained = ConstrainedMap::parse(map).unwrap();
                """,
            )
            writer.unitTest(
                name = "try_from_success",
                test = """
                    let map = build_valid_map();
                    let _constrained: ConstrainedMap = map.try_into().unwrap();
                """,
            )
            writer.unitTest(
                name = "parse_fail",
                test = """
                    let map = build_invalid_map();
                    let _constrained = ConstrainedMap::parse(map).unwrap_err();
                """,
            )
            writer.unitTest(
                name = "try_from_fail",
                test = """
                    let map = build_invalid_map();
                    let constrained_res: Result<ConstrainedMap, _> = map.try_into();
                    constrained_res.unwrap_err();
                """,
            )
            writer.unitTest(
                name = "inner",
                test = """
                    let map = build_valid_map();
                    let constrained = ConstrainedMap::parse(map.clone()).unwrap();

                    assert_eq!(constrained.inner(), &map);
                """,
            )
            writer.unitTest(
                name = "into_inner",
                test = """
                    let map = build_valid_map();
                    let constrained = ConstrainedMap::parse(map.clone()).unwrap();

                    assert_eq!(constrained.into_inner(), map);
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
            map ConstrainedMap {
                key: String,
                value: String
            }
            """.asSmithyModel()
        val constrainedMapShape = model.lookup<MapShape>("test#ConstrainedMap")

        val writer = RustWriter.forModule(ModelsModule.name)

        val codegenContext = serverTestCodegenContext(model)
        render(codegenContext, writer, constrainedMapShape)

        // Check that the wrapped type is `pub(crate)`.
        writer.toString() shouldContain "pub struct ConstrainedMap(pub(crate) std::collections::HashMap<std::string::String, std::string::String>);"
    }

    private fun render(
        codegenContext: ServerCodegenContext,
        writer: RustWriter,
        constrainedMapShape: MapShape,
    ) {
        ConstrainedMapGenerator(codegenContext, writer, constrainedMapShape).render()
        MapConstraintViolationGenerator(codegenContext, writer, constrainedMapShape).render()
    }
}
