/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.TestWorkspace
import software.amazon.smithy.rust.testutil.asSmithyModel
import software.amazon.smithy.rust.testutil.compileAndTest
import software.amazon.smithy.rust.testutil.testSymbolProvider
import software.amazon.smithy.rust.testutil.unitTest

class StreamingShapeTest {
    val _model = """
    namespace test
    operation StreamingOperation {
        output: StreamingOutput,
    }

    structure StreamingOutput {
        @required
        streamId: String,
        output: StreamingBlob,
    }

    @streaming
    blob StreamingBlob
    """.asSmithyModel()
    val model = OperationNormalizer(_model).transformModel(OperationNormalizer.NoBody, OperationNormalizer.NoBody)

    @Test
    fun `generate a byte stream member for the streaming output`() {
        val symbolProvider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(symbolProvider)
        val shape =
            model.lookup<StructureShape>("test#StreamingOperationOutput")
        project.useShapeWriter(shape) {
            StructureGenerator(model, symbolProvider = symbolProvider, it, shape).render()
            it.unitTest(
                """
                use smithy_stream::ByteStream;
                // verify that it is generated with a non-optional bytestream
                let _ = StreamingOperationOutput {
                    stream_id: None,
                    output: ByteStream::new("hello!")
                };
                """
            )
        }
        project.compileAndTest()
    }
}
