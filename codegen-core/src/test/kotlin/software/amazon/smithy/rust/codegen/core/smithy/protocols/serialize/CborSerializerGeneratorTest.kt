/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolContentTypes
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.lookup

class CborSerializerGeneratorTest {
    private val modelWithBigInteger =
        """
        namespace test
        use smithy.protocols#rpcv2Cbor

        @rpcv2Cbor
        service TestService {
            version: "test",
            operations: [TestOp]
        }

        structure TestInput {
            bigInt: BigInteger
        }

        @http(uri: "/test", method: "POST")
        operation TestOp {
            input: TestInput
        }
        """.asSmithyModel()

    private val modelWithBigDecimal =
        """
        namespace test
        use smithy.protocols#rpcv2Cbor

        @rpcv2Cbor
        service TestService {
            version: "test",
            operations: [TestOp]
        }

        structure TestInput {
            bigDec: BigDecimal
        }

        @http(uri: "/test", method: "POST")
        operation TestOp {
            input: TestInput
        }
        """.asSmithyModel()

    @Test
    fun `throws CodegenException when serializing BigInteger with CBOR`() {
        val model = OperationNormalizer.transform(modelWithBigInteger)
        val codegenContext = testCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider
        val serializerGenerator =
            CborSerializerGenerator(
                codegenContext,
                HttpTraitHttpBindingResolver(model, ProtocolContentTypes.consistent("application/cbor")),
            )
        val operationGenerator = serializerGenerator.operationInputSerializer(model.lookup("test#TestOp"))

        val project = TestWorkspace.testProject(symbolProvider)

        val exception =
            assertThrows<CodegenException> {
                project.lib {
                    unitTest(
                        "cbor_serializer",
                        """
                        let input = crate::test_input::TestOpInput::builder().build();
                        let _serialized = ${format(operationGenerator!!)}(&input);
                        """,
                    )
                }

                model.lookup<OperationShape>("test#TestOp").inputShape(model).also { input ->
                    input.renderWithModelBuilder(model, symbolProvider, project)
                }
                project.compileAndTest()
            }

        assert(exception.message!!.contains("BigInteger is not supported with Concise Binary Object Representation (CBOR)"))
    }

    @Test
    fun `throws CodegenException when serializing BigDecimal with CBOR`() {
        val model = OperationNormalizer.transform(modelWithBigDecimal)
        val codegenContext = testCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider
        val serializerGenerator =
            CborSerializerGenerator(
                codegenContext,
                HttpTraitHttpBindingResolver(model, ProtocolContentTypes.consistent("application/cbor")),
            )
        val operationGenerator = serializerGenerator.operationInputSerializer(model.lookup("test#TestOp"))

        val project = TestWorkspace.testProject(symbolProvider)

        val exception =
            assertThrows<CodegenException> {
                project.lib {
                    unitTest(
                        "cbor_serializer",
                        """
                        let input = crate::test_input::TestOpInput::builder().build();
                        let _serialized = ${format(operationGenerator!!)}(&input);
                        """,
                    )
                }

                model.lookup<OperationShape>("test#TestOp").inputShape(model).also { input ->
                    input.renderWithModelBuilder(model, symbolProvider, project)
                }
                project.compileAndTest()
            }

        assert(exception.message!!.contains("BigDecimal is not supported with Concise Binary Object Representation (CBOR)"))
    }
}
