/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGeneratorTest.Companion.model
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.generatePluginContext
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

    private val rustCrate: RustCrate
    private val codegenContext: CodegenContext = testCodegenContext(model)

    init {
        val (context, _) = generatePluginContext(
            model,
            runtimeConfig = codegenContext.runtimeConfig,
        )
        rustCrate = RustCrate(
            context.fileManifest,
            codegenContext.symbolProvider,
            codegenContext.settings.codegenConfig,
            codegenContext.expectModuleDocProvider(),
        )
    }

    private fun reexportsWithEmptyModel() = reexportsWithMember()
    private fun reexportsWithMember(
        inputMember: String = "",
        outputMember: String = "",
        unionMember: String = "",
        additionalShape: String = "",
    ) = RustWriter.root().let { writer ->
        pubUseSmithyPrimitives(
            codegenContext,
            modelWithMember(inputMember, outputMember, unionMember, additionalShape),
            rustCrate,
        )(writer)
        writer.toString()
    }

    private fun assertDoesntHaveReexports(reexports: String, expectedTypes: List<String>) =
        expectedTypes.forEach { assertDoesntHaveReexports(reexports, it) }

    private fun assertDoesntHaveReexports(reexports: String, type: String) {
        if (reexports.contains(type)) {
            throw AssertionError("Expected $type to NOT be re-exported, but it was.")
        }
    }

    private fun assertHasReexports(reexports: String, expectedTypes: List<String>) =
        expectedTypes.forEach { assertHasReexport(reexports, it) }

    private fun assertHasReexport(reexports: String, type: String) {
        if (!reexports.contains(type)) {
            throw AssertionError("Expected $type to be re-exported. Re-exported types:\n$reexports")
        }
    }

    @Test
    fun `it re-exports Blob when a model uses blobs`() {
        this.assertDoesntHaveReexports(reexportsWithEmptyModel(), "::aws_smithy_types::Blob")
        assertHasReexport(reexportsWithMember(inputMember = "foo: Blob"), "::aws_smithy_types::Blob")
        assertHasReexport(reexportsWithMember(outputMember = "foo: Blob"), "::aws_smithy_types::Blob")
        assertHasReexport(
            reexportsWithMember(inputMember = "foo: SomeUnion", unionMember = "foo: Blob"),
            "::aws_smithy_types::Blob",
        )
        assertHasReexport(
            reexportsWithMember(outputMember = "foo: SomeUnion", unionMember = "foo: Blob"),
            "::aws_smithy_types::Blob",
        )
    }

    @Test
    fun `it re-exports DateTime when a model uses timestamps`() {
        this.assertDoesntHaveReexports(reexportsWithEmptyModel(), "aws_smithy_types::DateTime")
        assertHasReexport(reexportsWithMember(inputMember = "foo: Timestamp"), "::aws_smithy_types::DateTime")
        assertHasReexport(reexportsWithMember(outputMember = "foo: Timestamp"), "::aws_smithy_types::DateTime")
        assertHasReexport(
            reexportsWithMember(inputMember = "foo: SomeUnion", unionMember = "foo: Timestamp"),
            "::aws_smithy_types::DateTime",
        )
        assertHasReexport(
            reexportsWithMember(outputMember = "foo: SomeUnion", unionMember = "foo: Timestamp"),
            "::aws_smithy_types::DateTime",
        )
    }

    @Test
    fun `it re-exports ByteStream and AggregatedBytes when a model has streaming`() {
        val streamingTypes =
            listOf("::aws_smithy_types::byte_stream::ByteStream", "::aws_smithy_types::byte_stream::AggregatedBytes")
        val streamingShape = "@streaming blob Streaming"

        this.assertDoesntHaveReexports(reexportsWithEmptyModel(), streamingTypes)
        assertHasReexports(reexportsWithMember(additionalShape = streamingShape, inputMember = "m: Streaming"), streamingTypes)
        assertHasReexports(reexportsWithMember(additionalShape = streamingShape, outputMember = "m: Streaming"), streamingTypes)

        // Event streams don't re-export the normal streaming types
        this.assertDoesntHaveReexports(
            reexportsWithMember(
                additionalShape = "@streaming union EventStream { foo: SomeStruct }",
                inputMember = "m: EventStream",
            ),
            streamingTypes,
        )
        this.assertDoesntHaveReexports(
            reexportsWithMember(
                additionalShape = "@streaming union EventStream { foo: SomeStruct }",
                outputMember = "m: EventStream",
            ),
            streamingTypes,
        )
    }
}
