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
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.lookup

class QuerySerializerGeneratorTest {
    companion object {
        val unionWithUnitStructModel =
            """
            namespace test

            union TestUnion {
                unitMember: Unit
            }

            structure Unit {}

            @http(uri: "/test", method: "POST")
            operation TestOp {
                input: TestInput
            }

            structure TestInput {
                union: TestUnion
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
        model.lookup<StructureShape>("test#Unit").renderWithModelBuilder(model, symbolProvider, project)
        model.lookup<OperationShape>("test#TestOp").inputShape(model).renderWithModelBuilder(model, symbolProvider, project)

        project.moduleFor(model.lookup<UnionShape>("test#TestUnion")) {
            UnionGenerator(model, symbolProvider, this, model.lookup("test#TestUnion")).render()
        }

        // Generate the serialization module that will contain the union serialization code
        project.lib {
            unitTest(
                "test_query_union_serialization",
                """
                use test_model::{TestUnion, Unit};

                // Create a test input to actually use the serializer
                let input = crate::test_input::TestOpInput::builder()
                    .union(TestUnion::UnitMember(Unit::builder().build()))
                    .build()
                    .unwrap();

                // This will generate and use the serialization code that should not have unused variable warnings
                let _serialized = ${format(operationGenerator!!)};
                let _result = _serialized(&input);

                // Test that the code compiles and runs - this validates our fix works
                """,
            )
        }

        project.compileAndTest()
    }
}
