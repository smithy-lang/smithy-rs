/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.parse

import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.testutil.clientTestRustSettings
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.EventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestRequirements
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestTools
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestVariety
import software.amazon.smithy.rust.codegen.core.testutil.TestEventStreamProject
import java.util.stream.Stream

class UnmarshallTestCasesProvider : ArgumentsProvider {
    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        EventStreamTestModels.TEST_CASES.map { Arguments.of(it) }.stream()
}

class EventStreamUnmarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(UnmarshallTestCasesProvider::class)
    fun test(testCase: EventStreamTestModels.TestCase) {
        EventStreamTestTools.runTestCase(
            testCase,
            object : EventStreamTestRequirements<ClientCodegenContext> {
                override fun createCodegenContext(
                    model: Model,
                    symbolProvider: RustSymbolProvider,
                    serviceShape: ServiceShape,
                    protocolShapeId: ShapeId,
                    codegenTarget: CodegenTarget,
                ): ClientCodegenContext = ClientCodegenContext(
                    model,
                    symbolProvider,
                    serviceShape,
                    protocolShapeId,
                    clientTestRustSettings(),
                    CombinedClientCodegenDecorator(emptyList()),
                )

                override fun createSymbolProvider(model: Model): RustSymbolProvider = testSymbolProvider(model)

                override fun renderGenerator(
                    codegenContext: ClientCodegenContext,
                    project: TestEventStreamProject,
                    protocol: Protocol,
                ): RuntimeType {
                    fun builderSymbol(shape: StructureShape): Symbol = shape.builderSymbol(codegenContext.symbolProvider)
                    return EventStreamUnmarshallerGenerator(
                        protocol,
                        codegenContext,
                        project.operationShape,
                        project.streamShape,
                        ::builderSymbol,
                    ).render()
                }
            },
            CodegenTarget.CLIENT,
            EventStreamTestVariety.Unmarshall,
        )
    }
}
