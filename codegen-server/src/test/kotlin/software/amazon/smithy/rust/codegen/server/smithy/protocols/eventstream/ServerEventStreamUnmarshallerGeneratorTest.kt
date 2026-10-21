/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamUnmarshallTestCases.writeUnmarshallTestCases
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class ServerEventStreamUnmarshallerGeneratorTest {
    @Test
    fun `legacy implicit event payloads validate enum builders`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test
            use aws.protocols#restJson1
            @restJson1
            service TestService { version: "1", operations: [TestStreamOp] }
            @http(method: "POST", uri: "/stream")
            operation TestStreamOp {
                input := { @required @httpPayload value: TestStream }
                output := { @required @httpPayload value: TestStream }
            }
            @streaming
            union TestStream { implicit: ImplicitEvent }
            structure ImplicitEvent {
                @eventHeader header: String
                kind: Kind
            }
            enum Kind {
                VALID = "valid"
            }
            """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ServerAdditionalSettings.builder().schemaSerde(false).toObjectNode(),
                cargoCommand = "cargo test --quiet --lib --tests",
            ),
        ) { context, crate ->
            crate.testModule {
                unitTest("implicit_event_enum_validation_preserves_headers") {
                    rustTemplate(
                        """
                        let unconstrained = crate::unconstrained::test_stream_unconstrained::TestStreamUnconstrained::Implicit(#{Default}::default());
                        let completed: #{Result}<crate::model::TestStream, _> = unconstrained.try_into();
                        assert!(completed.is_ok());
                        let unmarshaller = crate::event_stream_serde::TestStreamUnmarshaller::new();
                        for (payload, valid) in [(r##"{"kind":"valid"}"##, true), (r##"{"kind":"invalid"}"##, false)] {
                            let message = #{Types}::event_stream::Message::new_from_parts(
                                vec![
                                    #{Types}::event_stream::Header::new(":message-type", #{Types}::event_stream::HeaderValue::String("event".into())),
                                    #{Types}::event_stream::Header::new(":event-type", #{Types}::event_stream::HeaderValue::String("implicit".into())),
                                    #{Types}::event_stream::Header::new(":content-type", #{Types}::event_stream::HeaderValue::String("application/json".into())),
                                    #{Types}::event_stream::Header::new("header", #{Types}::event_stream::HeaderValue::String("hv".into())),
                                ],
                                #{Bytes}::copy_from_slice(payload.as_bytes()),
                            );
                            let result = #{EventStream}::frame::UnmarshallMessage::unmarshall(&unmarshaller, &message);
                            if valid {
                                match result.unwrap() {
                                    #{EventStream}::frame::UnmarshalledMessage::Event(crate::model::TestStream::Implicit(event)) => {
                                        assert_eq!(event.header.as_deref(), #{Some}("hv"));
                                        assert_eq!(event.kind.unwrap().as_str(), "valid");
                                    }
                                    _ => panic!("expected an implicit event"),
                                }
                            } else {
                                assert!(result.is_err(), "invalid enum values must be rejected");
                            }
                        }
                        """,
                        *RuntimeType.preludeScope,
                        "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                        "Bytes" to RuntimeType.Bytes,
                        "EventStream" to RuntimeType.smithyEventStream(context.runtimeConfig),
                    )
                }
            }
        }
    }

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
