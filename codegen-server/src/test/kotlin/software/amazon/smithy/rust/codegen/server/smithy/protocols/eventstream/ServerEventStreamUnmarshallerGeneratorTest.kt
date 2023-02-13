/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamUnmarshallTestCases.writeUnmarshallTestCases
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class RefactoredServerEventStreamUnmarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(TestCasesProvider::class)
    fun some_tests(testCase: TestCase) {
        // TODO(https://github.com/awslabs/smithy-rs/issues/1442): Enable tests for `publicConstrainedTypes = false`
        // by deleting this if/return
        if (!testCase.publicConstrainedTypes) {
            return
        }

        serverIntegrationTest(
            testCase.eventStreamTestCase.model,
            IntegrationTestParams(service = "test#TestService", addModuleToEventStreamAllowList = true),
        ) { codegenContext, rustCrate ->
            val crateName = codegenContext.moduleUseName()
            val generator = "$crateName::event_stream_serde::TestStreamUnmarshaller"

            rustCrate.integrationTest("unmarshall") {
                writeUnmarshallTestCases(
                    testCase.eventStreamTestCase,
                    codegenTarget = CodegenTarget.SERVER,
                    generator,
                    codegenContext,
                )
            }
        }
    }
}
