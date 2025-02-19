/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamUnmarshallTestCases.writeUnmarshallTestCases
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class ServerEventStreamUnmarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(TestCasesProvider::class)
    fun test(testCase: TestCase) {
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/1442): Enable tests for `publicConstrainedTypes = false`
        // by deleting this if/return
        if (!testCase.publicConstrainedTypes) {
            return
        }

        serverIntegrationTest(
            testCase.eventStreamTestCase.model,
            IntegrationTestParams(service = "test#TestService"),
        ) { codegenContext, rustCrate ->
            rustCrate.testModule {
                writeUnmarshallTestCases(
                    codegenContext,
                    testCase.eventStreamTestCase,
                    optionalBuilderInputs = true,
                )
            }
        }
    }
}
