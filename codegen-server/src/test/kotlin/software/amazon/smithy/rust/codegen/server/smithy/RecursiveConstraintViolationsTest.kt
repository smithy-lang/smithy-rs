/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.util.stream.Stream

internal class RecursiveConstraintViolationsTest {

    data class TestCase(
        /** The test name is only used in the generated report, to easily identify a failing test. **/
        val testName: String,
        /** The model to generate **/
        val model: Model,
        /** The shape ID of the member shape that should have the marker trait attached. **/
        val shapeIdWithConstraintViolationRustBoxTrait: String,
    )

    class RecursiveConstraintViolationsTestProvider : ArgumentsProvider {
        private val baseModel =
            """
            namespace com.amazonaws.recursiveconstraintviolations

            use aws.protocols#restJson1
            use smithy.framework#ValidationException

            @restJson1
            service RecursiveConstraintViolations {
                operations: [
                    Operation
                ]
            }

            @http(uri: "/operation", method: "POST")
            operation Operation {
                input: Recursive
                output: Recursive
                errors: [ValidationException]
            }
            """

        private fun recursiveListModel(sparse: Boolean, listPrefix: String = ""): Pair<Model, String> =
            """
            $baseModel

            structure Recursive {
                list: ${listPrefix}List
            }

            ${ if (sparse) { "@sparse" } else { "" } }
            @length(min: 69)
            list ${listPrefix}List {
                member: Recursive
            }
            """.asSmithyModel() to if ("${listPrefix}List" < "Recursive") {
                "com.amazonaws.recursiveconstraintviolations#${listPrefix}List\$member"
            } else {
                "com.amazonaws.recursiveconstraintviolations#Recursive\$list"
            }

        private fun recursiveMapModel(sparse: Boolean, mapPrefix: String = ""): Pair<Model, String> =
            """
            $baseModel

            structure Recursive {
                map: ${mapPrefix}Map
            }

            ${ if (sparse) { "@sparse" } else { "" } }
            @length(min: 69)
            map ${mapPrefix}Map {
                key: String,
                value: Recursive
            }
            """.asSmithyModel() to if ("${mapPrefix}Map" < "Recursive") {
                "com.amazonaws.recursiveconstraintviolations#${mapPrefix}Map\$value"
            } else {
                "com.amazonaws.recursiveconstraintviolations#Recursive\$map"
            }

        private fun recursiveUnionModel(unionPrefix: String = ""): Pair<Model, String> =
            """
            $baseModel

            structure Recursive {
                attributeValue: ${unionPrefix}AttributeValue
            }

            // Named `${unionPrefix}AttributeValue` in honor of DynamoDB's famous `AttributeValue`.
            // https://docs.rs/aws-sdk-dynamodb/latest/aws_sdk_dynamodb/model/enum.AttributeValue.html
            union ${unionPrefix}AttributeValue {
                set: SetAttribute
            }

            @uniqueItems
            list SetAttribute {
                member: ${unionPrefix}AttributeValue
            }
            """.asSmithyModel() to
                // The first loop the algorithm picks out to fix turns out to be the `list <-> union` loop:
                //
                // ```
                // [
                //     ${unionPrefix}AttributeValue,
                //     ${unionPrefix}AttributeValue$set,
                //     SetAttribute,
                //     SetAttribute$member
                // ]
                // ```
                //
                // Upon which, after fixing it, the other loop (`structure <-> list <-> union`) already contains
                // indirection, so we disregard it.
                //
                // This is hence a good test in that it tests that `RecursiveConstraintViolationBoxer` does not
                // superfluously add more indirection than strictly necessary.
                // However, it is a bad test in that if the Smithy library ever returns the recursive paths in a
                // different order, the (`structure <-> list <-> union`) loop might be fixed first, and this test might
                // start to fail! So watch out for that. Nonetheless, `RecursiveShapeBoxer` calls out:
                //
                //     This function MUST be deterministic (always choose the same shapes to `Box`). If it is not, that is a bug.
                //
                // So I think it's fair to write this test under the above assumption.
                if ("${unionPrefix}AttributeValue" < "SetAttribute") {
                    "com.amazonaws.recursiveconstraintviolations#${unionPrefix}AttributeValue\$set"
                } else {
                    "com.amazonaws.recursiveconstraintviolations#SetAttribute\$member"
                }

        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> {
            val listModels = listOf(false, true).flatMap { isSparse ->
                listOf("", "ZZZ").map { listPrefix ->
                    val (model, shapeIdWithConstraintViolationRustBoxTrait) = recursiveListModel(isSparse, listPrefix)
                    var testName = "${ if (isSparse) "sparse" else "non-sparse" } recursive list"
                    if (listPrefix.isNotEmpty()) {
                        testName += " with shape name prefix $listPrefix"
                    }
                    TestCase(testName, model, shapeIdWithConstraintViolationRustBoxTrait)
                }
            }
            val mapModels = listOf(false, true).flatMap { isSparse ->
                listOf("", "ZZZ").map { mapPrefix ->
                    val (model, shapeIdWithConstraintViolationRustBoxTrait) = recursiveMapModel(isSparse, mapPrefix)
                    var testName = "${ if (isSparse) "sparse" else "non-sparse" } recursive map"
                    if (mapPrefix.isNotEmpty()) {
                        testName += " with shape name prefix $mapPrefix"
                    }
                    TestCase(testName, model, shapeIdWithConstraintViolationRustBoxTrait)
                }
            }
            val unionModels = listOf("", "ZZZ").map { unionPrefix ->
                val (model, shapeIdWithConstraintViolationRustBoxTrait) = recursiveUnionModel(unionPrefix)
                var testName = "recursive union"
                if (unionPrefix.isNotEmpty()) {
                    testName += " with shape name prefix $unionPrefix"
                }
                TestCase(testName, model, shapeIdWithConstraintViolationRustBoxTrait)
            }
            return listOf(listModels, mapModels, unionModels)
                .flatten()
                .map { Arguments.of(it) }.stream()
        }
    }

    /**
     * Ensures the models generate code that compiles.
     *
     * Make sure the tests in [software.amazon.smithy.rust.codegen.server.smithy.transformers.RecursiveConstraintViolationBoxerTest]
     * are all passing before debugging any of these tests, since the former tests test preconditions for these.
     */
    @ParameterizedTest
    @ArgumentsSource(RecursiveConstraintViolationsTestProvider::class)
    fun `recursive constraint violation code generation test`(testCase: TestCase) {
        serverIntegrationTest(testCase.model)
    }
}
