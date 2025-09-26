/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.TestWriterDelegator
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.rustSettings
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.runCommand

class QuerySerializerGeneratorTest {
    companion object {
        val unionWithUnitStructModel =
            """
            namespace test

            union EncryptionFilter {
                none: NoneFilter,
                aes: AesFilter
            }

            structure NoneFilter {}

            structure AesFilter {
                keyId: String
            }

            @http(uri: "/test", method: "POST")
            operation TestOp {
                input: TestOpInput
            }

            structure TestOpInput {
                filter: EncryptionFilter
            }
            """.asSmithyModel()
    }

    @Test
    fun `union with unit struct doesn't cause unused variable warning`() {
        // Regression test for https://github.com/smithy-lang/smithy-rs/issues/4308
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(unionWithUnitStructModel))

        val codegenContext = testCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider
        val project = TestWorkspace.testProject(symbolProvider)

        val querySerializer = Ec2QuerySerializerGenerator(codegenContext)
        val operationGenerator = querySerializer.operationInputSerializer(model.lookup("test#TestOp"))

        // Render all necessary structures and unions
        model.lookup<StructureShape>("test#NoneFilter").renderWithModelBuilder(model, symbolProvider, project)
        model.lookup<StructureShape>("test#AesFilter").renderWithModelBuilder(model, symbolProvider, project)
        model.lookup<OperationShape>("test#TestOp").inputShape(model).renderWithModelBuilder(model, symbolProvider, project)

        project.moduleFor(model.lookup<UnionShape>("test#EncryptionFilter")) {
            UnionGenerator(model, symbolProvider, this, model.lookup("test#EncryptionFilter")).render()
        }

        // Generate the serialization module that will contain the union serialization code
        project.lib {
            unitTest(
                "test_query_union_serialization",
                """
                use test_model::{EncryptionFilter, NoneFilter};

                // Create a test input using unit struct pattern that causes unused variable warnings
                let input = crate::test_input::TestOpInput::builder()
                    .filter(EncryptionFilter::None(NoneFilter::builder().build()))
                    .build()
                    .unwrap();

                let serialized = ${format(operationGenerator!!)}(&input).unwrap();
                let output = std::str::from_utf8(serialized.bytes().unwrap()).unwrap();
                assert!(!output.is_empty());
                """,
            )
        }

        // Compile with warnings as errors to ensure no unused variable warnings
        compileWithWarningsAsErrors(project)
    }

    private fun compileWithWarningsAsErrors(project: TestWriterDelegator) {
        val stubModel =
            """
            namespace fake
            service Fake {
                version: "123"
            }
            """.asSmithyModel()

        project.finalize(
            project.rustSettings(),
            stubModel,
            manifestCustomizations = emptyMap(),
            libRsCustomizations = listOf(),
        )

        try {
            "cargo fmt".runCommand(project.baseDir)
        } catch (e: Exception) {
            // cargo fmt errors are useless, ignore
        }

        // Use RUSTFLAGS to treat unused variable warnings as errors, but allow dead code
        val env = mapOf("RUSTFLAGS" to "-D unused-variables -A dead-code")
        "cargo test".runCommand(project.baseDir, env)
    }
}
