/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.EventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestTools
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestVariety
import software.amazon.smithy.rust.codegen.core.testutil.TestEventStreamProject
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.serverBuilderSymbol

class ServerEventStreamUnmarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(TestCasesProvider::class)
    fun test(testCase: TestCase) {
        // TODO(https://github.com/awslabs/smithy-rs/issues/1442): Enable tests for `publicConstrainedTypes = false`
        // by deleting this if/return
        if (!testCase.publicConstrainedTypes) {
            return
        }

        EventStreamTestTools.runTestCase(
            testCase.eventStreamTestCase,
            object : ServerEventStreamBaseRequirements() {
                override val publicConstrainedTypes: Boolean get() = testCase.publicConstrainedTypes

                override fun renderGenerator(
                    codegenContext: ServerCodegenContext,
                    project: TestEventStreamProject,
                    protocol: Protocol,
                ): RuntimeType {
                    fun builderSymbol(shape: StructureShape): Symbol = shape.serverBuilderSymbol(codegenContext)
                    return EventStreamUnmarshallerGenerator(
                        protocol,
                        codegenContext,
                        project.operationShape,
                        project.streamShape,
                        ::builderSymbol,
                    ).render()
                }

                // TODO(https://github.com/awslabs/smithy-rs/issues/1442): Delete this function override to use the correct builder from the parent class
                override fun renderBuilderForShape(
                    writer: RustWriter,
                    codegenContext: ServerCodegenContext,
                    shape: StructureShape,
                ) {
                    BuilderGenerator(codegenContext.model, codegenContext.symbolProvider, shape).apply {
                        render(writer)
                        writer.implBlock(shape, codegenContext.symbolProvider) {
                            renderConvenienceMethod(writer)
                        }
                    }
                }
            },
            CodegenTarget.SERVER,
            EventStreamTestVariety.Unmarshall,
        )
    }
}
