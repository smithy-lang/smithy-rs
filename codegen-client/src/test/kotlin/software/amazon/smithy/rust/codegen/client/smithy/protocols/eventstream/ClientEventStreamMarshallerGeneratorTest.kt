/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.eventstream

import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestTools
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestVariety
import software.amazon.smithy.rust.codegen.core.testutil.TestEventStreamProject
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import java.util.stream.Stream

class MarshallTestCasesProvider : ArgumentsProvider {
    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        // Don't include awsQuery or ec2Query for now since marshall support for them is unimplemented
        EventStreamTestModels.TEST_CASES
            .filter { testCase -> !testCase.protocolShapeId.contains("Query") }
            .map { Arguments.of(it) }.stream()
}

class ClientEventStreamMarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(MarshallTestCasesProvider::class)
    fun test(testCase: EventStreamTestModels.TestCase) {
        EventStreamTestTools.runTestCase(
            testCase,
            object : ClientEventStreamBaseRequirements() {
                override fun renderGenerator(
                    codegenContext: ClientCodegenContext,
                    project: TestEventStreamProject,
                    protocol: Protocol,
                ): RuntimeType = EventStreamMarshallerGenerator(
                    project.model,
                    CodegenTarget.CLIENT,
                    TestRuntimeConfig,
                    project.symbolProvider,
                    project.streamShape,
                    protocol.structuredDataSerializer(project.operationShape),
                    testCase.requestContentType,
                ).render()
            },
            CodegenTarget.CLIENT,
            EventStreamTestVariety.Marshall,
        )
    }
}
