/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.serialize

import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestRequirements
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestTools
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestVariety
import software.amazon.smithy.rust.codegen.core.testutil.TestEventStreamProject
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.server.smithy.RustCodegenServerPlugin
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerSymbolProviders
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerTestSymbolVisitorConfig
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestRustSettings
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import java.util.stream.Stream

class MarshallTestCasesProvider : ArgumentsProvider {
    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        // Don't include awsQuery or ec2Query for now since marshall support for them is unimplemented
        EventStreamTestModels.TEST_CASES
            .filter { testCase -> !testCase.protocolShapeId.contains("Query") }
            .map { Arguments.of(it) }.stream()
}

class EventStreamMarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(MarshallTestCasesProvider::class)
    fun test(testCase: EventStreamTestModels.TestCase) {
        EventStreamTestTools.runTestCase(
            testCase,
            object : EventStreamTestRequirements<ServerCodegenContext> {
                override fun createCodegenContext(
                    model: Model,
                    symbolProvider: RustSymbolProvider,
                    serviceShape: ServiceShape,
                    protocolShapeId: ShapeId,
                    codegenTarget: CodegenTarget,
                ): ServerCodegenContext {
                    val settings = serverTestRustSettings()
                    val serverSymbolProviders = ServerSymbolProviders.from(
                        model,
                        serviceShape,
                        ServerTestSymbolVisitorConfig,
                        settings.codegenConfig.publicConstrainedTypes,
                        RustCodegenServerPlugin::baseSymbolProvider,
                    )
                    return ServerCodegenContext(
                        model,
                        symbolProvider,
                        serviceShape,
                        protocolShapeId,
                        settings,
                        serverSymbolProviders.unconstrainedShapeSymbolProvider,
                        serverSymbolProviders.constrainedShapeSymbolProvider,
                        serverSymbolProviders.constraintViolationSymbolProvider,
                        serverSymbolProviders.pubCrateConstrainedShapeSymbolProvider,
                    )
                }

                override fun createSymbolProvider(model: Model): RustSymbolProvider =
                    serverTestSymbolProvider(model)

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
            },
            CodegenTarget.SERVER,
            EventStreamTestVariety.Marshall,
        )
    }
}
