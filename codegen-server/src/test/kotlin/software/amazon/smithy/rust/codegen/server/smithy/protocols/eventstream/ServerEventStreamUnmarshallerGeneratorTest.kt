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
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.EventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestTools
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestVariety
import software.amazon.smithy.rust.codegen.core.testutil.TestEventStreamProject
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.serverBuilderSymbol
import java.util.stream.Stream

data class TestCase(
    val eventStreamTestCase: EventStreamTestModels.TestCase,
    val publicConstrainedTypes: Boolean,
) {
    override fun toString(): String = "$eventStreamTestCase, publicConstrainedTypes = $publicConstrainedTypes"
}

class UnmarshallTestCasesProvider : ArgumentsProvider {
    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        EventStreamTestModels.TEST_CASES.flatMap { testCase ->
            listOf(
                // TODO(https://github.com/awslabs/smithy-rs/issues/1442): Enable tests for `publicConstrainedTypes = false`
                // TestCase(testCase, false),
                TestCase(testCase, true),
            )
        }.map { Arguments.of(it) }.stream()
}

class ServerEventStreamUnmarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(UnmarshallTestCasesProvider::class)
    fun test(testCase: TestCase) {
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

                override fun renderBuilderForShape(
                    writer: RustWriter,
                    codegenContext: ServerCodegenContext,
                    shape: StructureShape,
                ) {
                    // TODO(https://github.com/awslabs/smithy-rs/issues/1442): Use the correct builder:
                    // ServerBuilderGenerator(codegenContext, shape).apply {
                    //     render(writer)
                    //     writer.implBlock(shape, codegenContext.symbolProvider) {
                    //         renderConvenienceMethod(writer)
                    //     }
                    // }
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
