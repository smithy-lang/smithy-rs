/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.eventstream

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamUnmarshallTestCases.generateRustPayloadInitializer
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamUnmarshallTestCases.writeUnmarshallTestCases
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

class ClientEventStreamUnmarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(TestCasesProvider::class)
    fun test(testCase: EventStreamTestModels.TestCase) {
        clientIntegrationTest(
            testCase.model,
            IntegrationTestParams(service = "test#TestService", addModuleToEventStreamAllowList = true),
        ) { codegenContext, rustCrate ->
            val generator = "crate::event_stream_serde::TestStreamUnmarshaller"

            rustCrate.testModule {
                rust("##![allow(unused_imports, dead_code)]")
                writeUnmarshallTestCases(codegenContext, testCase, optionalBuilderInputs = false)

                unitTest(
                    "unknown_message",
                    """
                    let message = msg("event", "NewUnmodeledMessageType", "application/octet-stream", b"hello, world!");
                    let result = $generator::new().unmarshall(&message);
                    assert!(result.is_ok(), "expected ok, got: {:?}", result);
                    assert!(expect_event(result.unwrap()).is_unknown());
                    """,
                )

                unitTest(
                    "generic_error",
                    """
                    let message = msg(
                        "exception",
                        "UnmodeledError",
                        "${testCase.responseContentType}",
                        ${testCase.generateRustPayloadInitializer(testCase.validUnmodeledError)}
                    );
                    let result = $generator::new().unmarshall(&message);
                    assert!(result.is_ok(), "expected ok, got: {:?}", result);
                    match expect_error(result.unwrap()) {
                        err @ TestStreamError::Unhandled(_) => {
                            let message = format!("{}", crate::error::DisplayErrorContext(&err));
                            let expected = "message: \"unmodeled error\"";
                            assert!(message.contains(expected), "Expected '{message}' to contain '{expected}'");
                        }
                        kind => panic!("expected generic error, but got {:?}", kind),
                    }
                    """,
                )
            }
        }
    }
}
