/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolContentTypes
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup

class CborSerializerGeneratorTest {
    @Test
    fun `union with unit struct doesn't cause unused variable warning`() {
        // Regression test for https://github.com/smithy-lang/smithy-rs/issues/4308
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

        val model = OperationNormalizer.transform(unionWithUnitStructModel)

        val codegenContext = testCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider
        val project = TestWorkspace.testProject(symbolProvider)

        val cborSerializer =
            CborSerializerGenerator(
                codegenContext,
                HttpTraitHttpBindingResolver(model, ProtocolContentTypes.consistent("application/cbor")),
            )

        model.lookup<StructureShape>("test#NoneFilter").renderWithModelBuilder(model, symbolProvider, project)
        model.lookup<StructureShape>("test#AesFilter").renderWithModelBuilder(model, symbolProvider, project)
        model.lookup<StructureShape>("test#TestOpInput").renderWithModelBuilder(model, symbolProvider, project)

        project.moduleFor(model.lookup<UnionShape>("test#EncryptionFilter")) {
            UnionGenerator(model, symbolProvider, this, model.lookup("test#EncryptionFilter")).render()
        }

        project.lib {
            unitTest(
                "cbor_union_serialization",
                """
                use test_model::{EncryptionFilter, NoneFilter};

                // This test verifies the generated union serialization code compiles
                // without unused variable warnings for empty structs
                let _filter = EncryptionFilter::None(NoneFilter {});
                """,
            )
        }

        project.compileAndTest()
    }
}
