/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGeneratorTest.Companion.model
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext

class SmithyTypesPubUseExtraTest {
    private fun modelWithMember(
        inputMember: String = "",
        outputMember: String = "",
        unionMember: String = "",
        additionalShape: String = "",
    ): Model {
        return """
            namespace test

            $additionalShape
            structure SomeStruct {
            }
            union SomeUnion {
                someStruct: SomeStruct,
                $unionMember
            }
            structure SomeInput {
                $inputMember
            }
            structure SomeOutput {
                $outputMember
            }

            operation SomeOperation {
                input: SomeInput,
                output: SomeOutput
            }
        """.asSmithyModel()
    }

    private fun typesWithEmptyModel() = typesWithMember()
    private fun typesWithMember(
        inputMember: String = "",
        outputMember: String = "",
        unionMember: String = "",
        additionalShape: String = "",
    ) = pubUseTypes(testCodegenContext(model), modelWithMember(inputMember, outputMember, unionMember, additionalShape))

    private fun assertDoesntHaveTypes(types: List<RuntimeType>, expectedTypes: List<String>) =
        expectedTypes.forEach { assertDoesntHaveType(types, it) }

    private fun assertDoesntHaveType(types: List<RuntimeType>, type: String) {
        if (types.any { t -> t.fullyQualifiedName() == type }) {
            throw AssertionError("Expected $type to NOT be re-exported, but it was.")
        }
    }

    private fun assertHasTypes(types: List<RuntimeType>, expectedTypes: List<String>) =
        expectedTypes.forEach { assertHasType(types, it) }

    private fun assertHasType(types: List<RuntimeType>, type: String) {
        if (types.none { t -> t.fullyQualifiedName() == type }) {
            throw AssertionError(
                "Expected $type to be re-exported. Re-exported types: " +
                    types.joinToString { it.fullyQualifiedName() },
            )
        }
    }

    @Test
    fun `it re-exports Blob when a model uses blobs`() {
        assertDoesntHaveType(typesWithEmptyModel(), "::aws_smithy_types::Blob")
        assertHasType(typesWithMember(inputMember = "foo: Blob"), "::aws_smithy_types::Blob")
        assertHasType(typesWithMember(outputMember = "foo: Blob"), "::aws_smithy_types::Blob")
        assertHasType(
            typesWithMember(inputMember = "foo: SomeUnion", unionMember = "foo: Blob"),
            "::aws_smithy_types::Blob",
        )
        assertHasType(
            typesWithMember(outputMember = "foo: SomeUnion", unionMember = "foo: Blob"),
            "::aws_smithy_types::Blob",
        )
    }

    @Test
    fun `it re-exports DateTime when a model uses timestamps`() {
        assertDoesntHaveType(typesWithEmptyModel(), "aws_smithy_types::DateTime")
        assertHasType(typesWithMember(inputMember = "foo: Timestamp"), "::aws_smithy_types::DateTime")
        assertHasType(typesWithMember(outputMember = "foo: Timestamp"), "::aws_smithy_types::DateTime")
        assertHasType(
            typesWithMember(inputMember = "foo: SomeUnion", unionMember = "foo: Timestamp"),
            "::aws_smithy_types::DateTime",
        )
        assertHasType(
            typesWithMember(outputMember = "foo: SomeUnion", unionMember = "foo: Timestamp"),
            "::aws_smithy_types::DateTime",
        )
    }

    @Test
    fun `it re-exports ByteStream and AggregatedBytes when a model has streaming`() {
        val streamingTypes =
            listOf("::aws_smithy_http::byte_stream::ByteStream", "::aws_smithy_http::byte_stream::AggregatedBytes")
        val streamingShape = "@streaming blob Streaming"

        assertDoesntHaveTypes(typesWithEmptyModel(), streamingTypes)
        assertHasTypes(typesWithMember(additionalShape = streamingShape, inputMember = "m: Streaming"), streamingTypes)
        assertHasTypes(typesWithMember(additionalShape = streamingShape, outputMember = "m: Streaming"), streamingTypes)

        // Event streams don't re-export the normal streaming types
        assertDoesntHaveTypes(
            typesWithMember(
                additionalShape = "@streaming union EventStream { foo: SomeStruct }",
                inputMember = "m: EventStream",
            ),
            streamingTypes,
        )
        assertDoesntHaveTypes(
            typesWithMember(
                additionalShape = "@streaming union EventStream { foo: SomeStruct }",
                outputMember = "m: EventStream",
            ),
            streamingTypes,
        )
    }
}
