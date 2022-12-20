/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestTools
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestVariety
import software.amazon.smithy.rust.codegen.core.testutil.TestEventStreamProject
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGenerator
import java.util.stream.Stream

class MarshallTestCasesProvider : ArgumentsProvider {
    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        // Don't include awsQuery or ec2Query for now since marshall support for them is unimplemented
        EventStreamTestModels.TEST_CASES
            .filter { testCase -> !testCase.protocolShapeId.contains("Query") }
            .map { Arguments.of(it) }.stream()
}

class ServerEventStreamMarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(MarshallTestCasesProvider::class)
    fun test(testCase: EventStreamTestModels.TestCase) {
        EventStreamTestTools.runTestCase(
            testCase,
            object : ServerEventStreamBaseRequirements() {
                override val publicConstrainedTypes: Boolean get() = true

                override fun renderGenerator(
                    codegenContext: ServerCodegenContext,
                    project: TestEventStreamProject,
                    protocol: Protocol,
                ): RuntimeType {
                    return EventStreamMarshallerGenerator(
                        project.model,
                        CodegenTarget.SERVER,
                        TestRuntimeConfig,
                        project.symbolProvider,
                        project.streamShape,
                        protocol.structuredDataSerializer(project.operationShape),
                        testCase.requestContentType,
                    ).render()
                }

                override fun renderBuilderForShape(
                    writer: RustWriter,
                    codegenContext: ServerCodegenContext,
                    shape: StructureShape,
                ) {
                    ServerBuilderGenerator(codegenContext, shape).apply {
                        render(writer)
                        writer.implBlock(shape, codegenContext.symbolProvider) {
                            renderConvenienceMethod(writer)
                        }
                    }
                }
            },
            CodegenTarget.SERVER,
            EventStreamTestVariety.Marshall,
        )
    }
}
