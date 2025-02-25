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
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.createTestInlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.transformers.ShapesReachableFromOperationInputTagger
import java.util.stream.Stream

@Suppress("DEPRECATION")
class ConstrainedCollectionGeneratorTest {
    data class TestCase(
        val model: Model,
        val validLists: List<ArrayNode>,
        val invalidLists: List<InvalidList>,
    )

    data class InvalidList(
        val node: ArrayNode,
        // A function returning a writable that renders the expected Rust value that the constructor error should
        // return.
        val expectedErrorFn: ((constraintViolation: Symbol, originalValueBindingName: String) -> Writable)?,
    )

    class ConstrainedListGeneratorTestProvider : ArgumentsProvider {
        private fun generateModel(trait: String): Model =
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
            """.asSmithyModel().let(ShapesReachableFromOperationInputTagger::transform)

        private val lengthTraitTestCases =
            listOf(
                // Min and max.
                Triple("@length(min: 11, max: 12)", 11, 13),
                // Min equal to max.
                Triple("@length(min: 11, max: 11)", 11, 12),
                // Only min.
                Triple("@length(min: 11)", 15, 10),
                // Only max.
                Triple("@length(max: 11)", 11, 12),
            ).map {
                // Generate lists of strings of the specified length with consecutive items "0", "1", ...
                val validList = List(it.second, Int::toString)
                val invalidList = List(it.third, Int::toString)

                Triple(it.first, ArrayNode.fromStrings(validList), ArrayNode.fromStrings(invalidList))
            }.map { (trait, validList, invalidList) ->
                TestCase(
                    model = generateModel(trait),
                    validLists = listOf(validList),
                    invalidLists = listOf(InvalidList(invalidList, expectedErrorFn = null)),
                )
            }

        private fun constraintViolationForDuplicateIndices(
            duplicateIndices: List<Int>,
        ): ((constraintViolation: Symbol, originalValueBindingName: String) -> Writable) {
            fun ret(
                constraintViolation: Symbol,
                originalValueBindingName: String,
            ): Writable =
                writable {
                    // Public documentation for the unique items constraint violation states that callers should not
                    // rely on the order of the elements in `duplicate_indices`. However, the algorithm is deterministic,
                    // so we can internally assert the order. If the algorithm changes, the test cases will need to be
                    // adjusted.
                    rustTemplate(
                        """
                        #{ConstraintViolation}::UniqueItems {
                            duplicate_indices: vec![${duplicateIndices.joinToString(", ")}],
                            original: $originalValueBindingName,
                        }
                        """,
                        "ConstraintViolation" to constraintViolation,
                    )
                }

            return ::ret
        }

        private val uniqueItemsTraitTestCases =
            listOf(
                // We only need one test case, since `@uniqueItems` is not parameterizable.
                TestCase(
                    model = generateModel("@uniqueItems"),
                    validLists =
                        listOf(
                            ArrayNode.fromStrings(),
                            ArrayNode.fromStrings("0", "1"),
                            ArrayNode.fromStrings("a", "b", "a2"),
                            ArrayNode.fromStrings((0..69).map(Int::toString).toList()),
                        ),
                    invalidLists =
                        listOf(
                            // Two elements, both duplicate.
                            InvalidList(
                                node = ArrayNode.fromStrings("0", "0"),
                                expectedErrorFn = constraintViolationForDuplicateIndices(listOf(0, 1)),
                            ),
                            // Two duplicate items, one at the beginning, one at the end.
                            InvalidList(
                                node = ArrayNode.fromStrings("0", "1", "2", "3", "4", "5", "0"),
                                expectedErrorFn = constraintViolationForDuplicateIndices(listOf(0, 6)),
                            ),
                            // Several duplicate items, all the same.
                            InvalidList(
                                node = ArrayNode.fromStrings("0", "1", "0", "0", "4", "0", "6", "7"),
                                expectedErrorFn = constraintViolationForDuplicateIndices(listOf(0, 2, 3, 5)),
                            ),
                            // Several equivalence classes.
                            InvalidList(
                                node = ArrayNode.fromStrings("0", "1", "0", "2", "1", "0", "2", "7", "2"),
                                // Note how the duplicate indices are not ordered.
                                expectedErrorFn = constraintViolationForDuplicateIndices(listOf(0, 1, 2, 3, 6, 5, 4, 8)),
                            ),
                            // The worst case: a fairly large number of elements, all duplicate.
                            InvalidList(
                                node = ArrayNode.fromStrings(generateSequence { "69" }.take(69).toList()),
                                expectedErrorFn = constraintViolationForDuplicateIndices((0..68).toList()),
                            ),
                        ),
                ),
            )

        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
            (lengthTraitTestCases + uniqueItemsTraitTestCases).map { Arguments.of(it) }.stream()
    }

    @ParameterizedTest
    @ArgumentsSource(ConstrainedListGeneratorTestProvider::class)
    fun `it should generate constrained collection types`(testCase: TestCase) {
        val constrainedListShape = testCase.model.lookup<CollectionShape>("test#ConstrainedList")
        val constrainedSetShape = testCase.model.lookup<CollectionShape>("test#ConstrainedSet")

        val codegenContext = serverTestCodegenContext(testCase.model)

        val project = TestWorkspace.testProject(codegenContext.symbolProvider)
        project.withModule(ServerRustModule.Model) {
            TestUtility.generateIsDisplay().invoke(this)
            TestUtility.generateIsError().invoke(this)
        }

        for (shape in listOf(constrainedListShape, constrainedSetShape)) {
            val shapeName =
                when (shape) {
                    is SetShape -> "set"
                    is ListShape -> "list"
                    else -> UNREACHABLE("Shape is either list or set.")
                }

            project.withModule(ServerRustModule.Model) {
                render(codegenContext, this, shape)

                val instantiator = ServerInstantiator(codegenContext)
                for ((idx, validList) in testCase.validLists.withIndex()) {
                    val shapeNameIdx = "${shapeName}_$idx"
                    val buildValidFnName = "build_valid_$shapeNameIdx"
                    val typeName = "Constrained${shapeName.replaceFirstChar { it.uppercaseChar() }}"

                    rustBlock("##[cfg(test)] fn $buildValidFnName() -> std::vec::Vec<std::string::String>") {
                        instantiator.render(this, shape, validList)
                    }

                    unitTest(
                        name = "${shapeNameIdx}_try_from_success",
                        test = """
                            let $shapeNameIdx = $buildValidFnName();
                            let _constrained: $typeName = $shapeNameIdx.try_into().unwrap();
                        """,
                    )
                    unitTest(
                        name = "${shapeNameIdx}_inner",
                        test = """
                            let $shapeNameIdx = $buildValidFnName();
                            let constrained = $typeName::try_from($shapeNameIdx.clone()).unwrap();

                            assert_eq!(constrained.inner(), &$shapeNameIdx);
                        """,
                    )
                    unitTest(
                        name = "${shapeNameIdx}_into_inner",
                        test = """
                            let $shapeNameIdx = $buildValidFnName();
                            let constrained = $typeName::try_from($shapeNameIdx.clone()).unwrap();

                            assert_eq!(constrained.into_inner(), $shapeNameIdx);
                        """,
                    )
                }

                for ((idx, invalidList) in testCase.invalidLists.withIndex()) {
                    val shapeNameIdx = "${shapeName}_$idx"
                    val buildInvalidFnName = "build_invalid_$shapeNameIdx"
                    val typeName = "Constrained${shapeName.replaceFirstChar { it.uppercaseChar() }}"

                    rustBlock("##[cfg(test)] fn $buildInvalidFnName() -> std::vec::Vec<std::string::String>") {
                        instantiator.render(this, shape, invalidList.node)
                    }
                    unitTest(
                        name = "${shapeNameIdx}_try_from_fail",
                        block =
                            writable {
                                rust(
                                    """
                                    let $shapeNameIdx = $buildInvalidFnName();
                                    let constrained_res: Result <$typeName, _> = $shapeNameIdx.clone().try_into();
                                    """,
                                )

                                invalidList.expectedErrorFn?.also { expectedErrorFn ->
                                    val expectedErrorWritable =
                                        expectedErrorFn(
                                            codegenContext.constraintViolationSymbolProvider.toSymbol(shape),
                                            shapeNameIdx,
                                        )

                                    rust("let err = constrained_res.unwrap_err();")
                                    withBlock("let expected_err = ", ";") {
                                        rustTemplate("#{ExpectedError:W}", "ExpectedError" to expectedErrorWritable)
                                    }
                                    rust(
                                        """
                                        assert_eq!(err, expected_err);
                                        is_error(&err);
                                        is_display(&err);
                                        // Ensure that the `std::fmt::Display` implementation for `ConstraintViolation` error works.
                                        assert_eq!(err.to_string(), expected_err.to_string());
                                        """.trimMargin(),
                                    )
                                } ?: run {
                                    rust("constrained_res.unwrap_err();")
                                }
                            },
                    )
                }
            }
        }

        project.compileAndTest()
    }

    @Test
    fun `type should not be constructable without using a constructor`() {
        val model =
            """
            namespace test

            @length(min: 1, max: 69)
            list ConstrainedList {
                member: String
            }
            """.asSmithyModel().let(ShapesReachableFromOperationInputTagger::transform)
        val constrainedCollectionShape = model.lookup<CollectionShape>("test#ConstrainedList")

        val writer = RustWriter.forModule(ServerRustModule.Model.name)

        val codegenContext = serverTestCodegenContext(model)
        render(codegenContext, writer, constrainedCollectionShape)

        // Check that the wrapped type is `pub(crate)`.
        writer.toString() shouldContain "pub struct ConstrainedList(pub(crate) ::std::vec::Vec<::std::string::String>);"
    }

    @Test
    fun `error trait implemented for ConstraintViolation should work for constrained member`() {
        val model =
            """
            ${'$'}version: "1.0"
            
            namespace test
            
            use aws.protocols#restJson1
            use smithy.framework#ValidationException
            
            // The `ConstraintViolation` code generated for a constrained map that is not reachable from an
            // operation does not have the `Key`, or `Value` variants. Hence, we need to define a service
            // and an operation that uses the constrained map.
            @restJson1
            service MyService {
                version: "2023-04-01",
                operations: [
                    MyOperation,
                ]
            }
            
            @http(method: "POST", uri: "/echo")
            operation MyOperation {
                input: MyOperationInput
                errors : [ValidationException]
            }
            
            @input
            structure MyOperationInput {
                    member1: ConstrainedList,
                    member2: ConstrainedSet,
            }            

            @length(min: 2, max: 69)
            list ConstrainedList {
                member: ConstrainedString
            }

            @length(min: 2, max: 69)
            set ConstrainedSet {
                member: ConstrainedString
            }

            @pattern("#\\d+")
            string ConstrainedString
            """.asSmithyModel().let(ShapesReachableFromOperationInputTagger::transform)

        val codegenContext = serverTestCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider
        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(ServerRustModule.Model) {
            TestUtility.generateIsDisplay().invoke(this)
            TestUtility.generateIsError().invoke(this)
            TestUtility.renderConstrainedString(
                codegenContext, this,
                model.lookup<StringShape>("test#ConstrainedString"),
            )

            rustTemplate(
                """
                // Define `ValidationExceptionField` since it is required by the `ConstraintViolation` code for constrained maps, 
                // and the complete SDK generation process, which would generate it, is not invoked as part of the test.
                pub struct ValidationExceptionField {
                    pub message: String,
                    pub path: String
                }
                """,
                *RuntimeType.preludeScope,
            )

            val constrainedListShape = model.lookup<ListShape>("test#ConstrainedList")
            val constrainedSetShape = model.lookup<ListShape>("test#ConstrainedSet")
            render(codegenContext, this, constrainedListShape)
            render(codegenContext, this, constrainedSetShape)

            unitTest(
                name = "try_from_fail_invalid_constrained_list",
                test = """
                    let constrained_error = ConstrainedString::try_from("one".to_string()).unwrap_err();
                    let error = crate::model::constrained_list::ConstraintViolation::Member(0, constrained_error);
                    is_error(&error);
                    is_display(&error);
                    assert_eq!("Value at index 0 failed to satisfy constraint. Value provided for `test#ConstrainedString` failed to satisfy the constraint: Member must match the regular expression pattern: #\\d+", 
                        error.to_string());
                """,
            )
            unitTest(
                name = "try_from_fail_invalid_constrained_set",
                test = """
                    let constrained_error = ConstrainedString::try_from("one".to_string()).unwrap_err();
                    let error = crate::model::constrained_set::ConstraintViolation::Member(0, constrained_error);
                    is_error(&error);
                    is_display(&error);
                    assert_eq!("Value at index 0 failed to satisfy constraint. Value provided for `test#ConstrainedString` failed to satisfy the constraint: Member must match the regular expression pattern: #\\d+", 
                        error.to_string());
                """,
            )
        }

        project.compileAndTest()
    }

    private fun render(
        codegenContext: ServerCodegenContext,
        writer: RustWriter,
        constrainedCollectionShape: CollectionShape,
    ) {
        val constraintsInfo = CollectionTraitInfo.fromShape(constrainedCollectionShape, codegenContext.symbolProvider)
        ConstrainedCollectionGenerator(codegenContext, writer, constrainedCollectionShape, constraintsInfo).render()
        CollectionConstraintViolationGenerator(
            codegenContext,
            writer.createTestInlineModuleCreator(),
            constrainedCollectionShape,
            constraintsInfo,
            SmithyValidationExceptionConversionGenerator(codegenContext),
        ).render()
    }
}
