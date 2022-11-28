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
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.transformers.ShapesReachableFromOperationInputTagger
import java.util.stream.Stream

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
        val constrainedListShape = testCase.model.lookup<ListShape>("test#ConstrainedList")

        val codegenContext = serverTestCodegenContext(testCase.model)
        val symbolProvider = codegenContext.symbolProvider

        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(ModelsModule) {
            render(codegenContext, this, constrainedListShape)

            val instantiator = serverInstantiator(codegenContext)
            rustBlock("##[cfg(test)] fn build_valid_list() -> std::vec::Vec<std::string::String>") {
                instantiator.render(this, constrainedListShape, testCase.validList)
            }
            rustBlock("##[cfg(test)] fn build_invalid_list() -> std::vec::Vec<std::string::String>") {
                instantiator.render(this, constrainedListShape, testCase.invalidList)
            }

            unitTest(
                name = "try_from_success",
                test = """
                    let list = build_valid_list();
                    let _constrained: ConstrainedList = list.try_into().unwrap();
                """,
            )
            unitTest(
                name = "try_from_fail",
                test = """
                    let list = build_invalid_list();
                    let constrained_res: Result<ConstrainedList, _> = list.try_into();
                    constrained_res.unwrap_err();
                """,
            )
            unitTest(
                name = "inner",
                test = """
                    let list = build_valid_list();
                    let constrained = ConstrainedList::try_from(list.clone()).unwrap();

                    assert_eq!(constrained.inner(), &list);
                """,
            )
            unitTest(
                name = "into_inner",
                test = """
                    let list = build_valid_list();
                    let constrained = ConstrainedList::try_from(list.clone()).unwrap();

                    assert_eq!(constrained.into_inner(), list);
                """,
            )
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
