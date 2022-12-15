/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.serialize

import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestTools
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
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
            CodegenTarget.CLIENT,
            { model -> testSymbolProvider(model) },
            { _, test, protocol ->
                EventStreamMarshallerGenerator(
                    test.model,
                    CodegenTarget.CLIENT,
                    TestRuntimeConfig,
                    test.symbolProvider,
                    test.streamShape,
                    protocol.structuredDataSerializer(test.operationShape),
                    testCase.requestContentType,
                ).render()
            },
            marshall = true,
        )
    }
}
