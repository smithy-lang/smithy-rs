/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolContentTypes
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.SerializerGeneratorTestUtils.UnionWithEmptyStructShapeIds
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.SerializerGeneratorTestUtils.unionWithEmptyStructModel

class CborSerializerGeneratorTest {

    @Test
    fun `union with empty struct doesn't cause unused variable warning`() {
        // Regression test for https://github.com/smithy-lang/smithy-rs/issues/4308
        val model = OperationNormalizer.transform(unionWithEmptyStructModel)
        val codegenContext = testCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider
        val parserGenerator = CborSerializerGenerator(
            codegenContext,
            HttpTraitHttpBindingResolver(model, ProtocolContentTypes.consistent("application/cbor"))
        )
        val operationGenerator = parserGenerator.operationInputSerializer(model.lookup(UnionWithEmptyStructShapeIds.TEST_OPERATION))

        val project = TestWorkspace.testProject(symbolProvider)
        project.lib {
            unitTest(
                "union_with_empty_struct_cbor_serialization",
                """
                use test_model::{TestUnion, EmptyStruct};

                let input = crate::test_input::TestOperationInput::builder()
                    .union(TestUnion::EmptyStructMember(EmptyStruct::builder().build()))
                    .build()
                    .unwrap();
                let _serialized = ${format(operationGenerator!!)}(&input).unwrap();

                let input = crate::test_input::TestOperationInput::builder()
                    .union(TestUnion::DataMember("test".to_string()))
                    .build()
                    .unwrap();
                let _serialized = ${format(operationGenerator)}(&input).unwrap();
                """,
            )
        }

        model.lookup<StructureShape>(UnionWithEmptyStructShapeIds.EMPTY_STRUCT).also { emptyStruct ->
            emptyStruct.renderWithModelBuilder(model, symbolProvider, project)
        }

        model.lookup<StructureShape>(UnionWithEmptyStructShapeIds.TEST_INPUT).also { testInput ->
            testInput.renderWithModelBuilder(model, symbolProvider, project)
            project.moduleFor(testInput) {
                UnionGenerator(model, symbolProvider, this, model.lookup(UnionWithEmptyStructShapeIds.TEST_UNION)).render()
            }
        }

        model.lookup<OperationShape>(UnionWithEmptyStructShapeIds.TEST_OPERATION).inputShape(model).also { input ->
            input.renderWithModelBuilder(model, symbolProvider, project)
        }

        project.compileAndTest()
    }
}
