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
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.transformers.ShapesReachableFromOperationInputTagger
import java.util.stream.Stream

@Suppress("DEPRECATION")
class ConstrainedCollectionGeneratorTest {
    data class TestCase(val model: Model, val validList: ArrayNode, val invalidList: ArrayNode)

    class ConstrainedListGeneratorTestProvider : ArgumentsProvider {
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
            val validList = List(it.second, Int::toString)
            val invalidList = List(it.third, Int::toString)

            Triple(it.first, ArrayNode.fromStrings(validList), ArrayNode.fromStrings(invalidList))
        }.map { (trait, validList, invalidList) ->
            TestCase(
                """
                namespace test

                $trait
                list ConstrainedList {
                    member: String
                }

                $trait
                set ConstrainedSet {
                    member: String
                }
                """.asSmithyModel().let(ShapesReachableFromOperationInputTagger::transform),
                validList,
                invalidList,
            )
        }

        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
            testCases.map { Arguments.of(it) }.stream()
    }

    @ParameterizedTest
    @ArgumentsSource(ConstrainedListGeneratorTestProvider::class)
    fun `it should generate constrained collection types`(testCase: TestCase) {
        val constrainedListShape = testCase.model.lookup<CollectionShape>("test#ConstrainedList")
        // TODO(https://github.com/awslabs/smithy-rs/issues/1401): a `set` shape is
        //  just a `list` shape with `uniqueItems`, which hasn't been implemented yet.
        // val constrainedSetShape = testCase.model.lookup<CollectionShape>("test#ConstrainedSet")

        val codegenContext = serverTestCodegenContext(testCase.model)
        val symbolProvider = codegenContext.symbolProvider

        val project = TestWorkspace.testProject(symbolProvider)

        listOf(constrainedListShape /*, constrainedSetShape */).forEach { shape ->
            val shapeName = when (shape) {
                is ListShape -> "list"
                is SetShape -> "set"
                else -> UNREACHABLE("Shape is either list or set.")
            }

            project.withModule(ModelsModule) {
                render(codegenContext, this, shape)

                val instantiator = serverInstantiator(codegenContext)
                rustBlock("##[cfg(test)] fn build_valid_$shapeName() -> std::vec::Vec<std::string::String>") {
                    instantiator.render(this, shape, testCase.validList)
                }
                rustBlock("##[cfg(test)] fn build_invalid_$shapeName() -> std::vec::Vec<std::string::String>") {
                    instantiator.render(this, shape, testCase.invalidList)
                }

                unitTest(
                    name = "try_from_success",
                    test = """
                        let $shapeName = build_valid_$shapeName();
                        let _constrained: ConstrainedList = $shapeName.try_into().unwrap();
                    """,
                )
                unitTest(
                    name = "try_from_fail",
                    test = """
                        let $shapeName = build_invalid_$shapeName();
                        let constrained_res: Result<ConstrainedList, _> = $shapeName.try_into();
                        constrained_res.unwrap_err();
                    """,
                )
                unitTest(
                    name = "inner",
                    test = """
                        let $shapeName = build_valid_$shapeName();
                        let constrained = ConstrainedList::try_from($shapeName.clone()).unwrap();

                        assert_eq!(constrained.inner(), &$shapeName);
                    """,
                )
                unitTest(
                    name = "into_inner",
                    test = """
                        let $shapeName = build_valid_$shapeName();
                        let constrained = ConstrainedList::try_from($shapeName.clone()).unwrap();

                        assert_eq!(constrained.into_inner(), $shapeName);
                    """,
                )
            }
        }

        project.compileAndTest()
    }

    @Test
    fun `type should not be constructible without using a constructor`() {
        val model = """
            namespace test

            @length(min: 1, max: 69)
            list ConstrainedList {
                member: String
            }
        """.asSmithyModel().let(ShapesReachableFromOperationInputTagger::transform)
        val constrainedCollectionShape = model.lookup<CollectionShape>("test#ConstrainedList")

        val writer = RustWriter.forModule(ModelsModule.name)

        val codegenContext = serverTestCodegenContext(model)
        render(codegenContext, writer, constrainedCollectionShape)

        // Check that the wrapped type is `pub(crate)`.
        writer.toString() shouldContain "pub struct ConstrainedList(pub(crate) std::vec::Vec<std::string::String>);"
    }

    private fun render(
        codegenContext: ServerCodegenContext,
        writer: RustWriter,
        constrainedCollectionShape: CollectionShape,
    ) {
        val constraintsInfo = CollectionTraitInfo.fromShape(constrainedCollectionShape)
        ConstrainedCollectionGenerator(codegenContext, writer, constrainedCollectionShape, constraintsInfo).render()
        CollectionConstraintViolationGenerator(codegenContext, writer, constrainedCollectionShape, constraintsInfo).render()
    }
}
