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
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamMarshallTestCases.writeMarshallTestCases
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.util.stream.Stream

class ServerEventStreamMarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(TestCasesProvider::class)
    fun test(testCase: TestCase) {
        serverIntegrationTest(testCase.eventStreamTestCase.model) { codegenContext, rustCrate ->
            rustCrate.testModule {
                writeMarshallTestCases(codegenContext, testCase.eventStreamTestCase, optionalBuilderInputs = true)
            }
        }
    }
}

data class TestCase(
    val eventStreamTestCase: EventStreamTestModels.TestCase,
    val publicConstrainedTypes: Boolean,
) {
    override fun toString(): String = "$eventStreamTestCase, publicConstrainedTypes = $publicConstrainedTypes"
}

class TestCasesProvider : ArgumentsProvider {
    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        EventStreamTestModels.TEST_CASES
            .flatMap { testCase ->
                listOf(
                    TestCase(testCase, publicConstrainedTypes = false),
                    TestCase(testCase, publicConstrainedTypes = true),
                )
            }.map { Arguments.of(it) }.stream()
}
