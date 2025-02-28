/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.eventstream

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamUnmarshallTestCases.generateRustPayloadInitializer
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamUnmarshallTestCases.writeUnmarshallTestCases
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamUnmarshallTestCases.writeUnmarshallTestUtil
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq

class ClientEventStreamUnmarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(TestCasesProvider::class)
    fun test(testCase: EventStreamTestModels.TestCase) {
        clientIntegrationTest(
            testCase.model,
            IntegrationTestParams(service = "test#TestService"),
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

    @ParameterizedTest
    @ArgumentsSource(RpcEventStreamTestCasesProvider::class)
    fun rpcEventStreamTest(rpcEventStreamTestCase: RpcEventStreamTestCase) {
        val defaultEventStreamReceiver =
            writable {
                rust(
                    """
                    if let Some(event) = response.value.recv().await.unwrap() {
                        match event {
                            TestStream::MessageWithString(message_with_string) => {
                                assert_eq!("hello, world!", message_with_string.data().unwrap())
                            }
                            otherwise => panic!("matched on unexpected variant {otherwise:?}"),
                        }
                    } else {
                        panic!("should receive at least one frame");
                    }
                    """,
                )
            }

        fun RustWriter.writeTestBody(
            codegenContext: CodegenContext,
            initialResponseStreamGenerator: Writable,
            eventStreamGenerator: Writable,
            sendResultHandler: Writable = writable { rust("let mut response = response.unwrap();") },
            initialResponseAssertion: Writable,
            eventStreamReceiver: Writable = defaultEventStreamReceiver,
        ) {
            rustTemplate(
                """
                use aws_smithy_eventstream::frame::{MarshallMessage, write_message_to};
                use aws_smithy_http::event_stream::EventStreamSender;
                use aws_smithy_types::body::SdkBody;
                use #{futures_util}::StreamExt;

                #{initial_response_stream:W}

                let event = TestStream::MessageWithString(
                    MessageWithString::builder().data("hello, world!").build(),
                );
                let event_stream_payload = crate::event_stream_serde::TestStreamMarshaller::new()
                    .marshall(event.clone())
                    .unwrap();
                let mut buffer = vec![];
                write_message_to(&event_stream_payload, &mut buffer).unwrap();
                // Error type here is arbitrary to satisfy the compiler
                let chunks: Vec<Result<bytes::Bytes, std::io::Error>> = vec![Ok(buffer.into())];

                #{event_stream:W}

                let (http_client, _r) = #{capture_request}(Some(
                    http::Response::builder()
                        .status(200)
                        .body(SdkBody::from_body_0_4(
                            hyper::Body::wrap_stream(event_stream),
                        ))
                        .unwrap(),
                ));
                let conf = crate::Config::builder()
                    .endpoint_url("http://localhost:1234")
                    .http_client(http_client.clone())
                    .behavior_version_latest()
                    .build();
                let client = crate::Client::from_conf(conf);

                // This is dummy stream to satisfy input validation for the test service
                let stream = #{futures_util}::stream::iter(vec![Ok(event)]);
                let response = client
                    .test_stream_op()
                    .value(EventStreamSender::from(stream))
                    .send()
                    .await;

                #{handle_send_result:W}

                #{assert_initial_response:W}

                #{receive_event_stream:W}
                """,
                "assert_initial_response" to initialResponseAssertion,
                "capture_request" to RuntimeType.captureRequest(codegenContext.runtimeConfig),
                "event_stream" to eventStreamGenerator,
                "futures_util" to CargoDependency.FuturesUtil.toType(),
                "handle_send_result" to sendResultHandler,
                "initial_response_stream" to initialResponseStreamGenerator,
                "receive_event_stream" to eventStreamGenerator,
                "receive_event_stream" to eventStreamReceiver,
            )
        }

        clientIntegrationTest(
            rpcEventStreamTestCase.inner.model,
            IntegrationTestParams(service = "test#TestService"),
        ) { codegenContext, rustCrate ->
            rustCrate.testModule {
                rust("##![allow(unused_imports, dead_code)]")
                writeUnmarshallTestUtil(codegenContext)

                when (rpcEventStreamTestCase.nonEventStreamMember) {
                    NonEventStreamMemberInOutput.OPTIONAL_SET, NonEventStreamMemberInOutput.REQUIRED -> {
                        tokioTest("with_initial_response") {
                            writeTestBody(
                                codegenContext,
                                initialResponseStreamGenerator =
                                    writable {
                                        val testCase = rpcEventStreamTestCase.inner
                                        rust(
                                            """
                                            let initial_response = msg(
                                                "event",
                                                "initial-response",
                                                ${testCase.eventStreamMessageContentType.dq()},
                                                ${testCase.generateRustPayloadInitializer(testCase.eventStreamInitialResponsePayload!!)},
                                            );
                                            let mut buffer = vec![];
                                            aws_smithy_eventstream::frame::write_message_to(&initial_response, &mut buffer).unwrap();
                                            let chunks: Vec<Result<bytes::Bytes, _>> = vec![Ok(buffer.into())];
                                            let initial_response_stream = futures_util::stream::iter(chunks);
                                            """,
                                        )
                                    },
                                eventStreamGenerator =
                                    writable {
                                        rust("let event_stream = initial_response_stream.chain(futures_util::stream::iter(chunks));")
                                    },
                                initialResponseAssertion =
                                    writable {
                                        rust("assert_eq!(${rpcEventStreamTestCase.expectedInInitialResponse}, response.test_string());")
                                    },
                            )
                        }
                        tokioTest("with_initial_response_containing_invalid_data") {
                            writeTestBody(
                                codegenContext,
                                initialResponseStreamGenerator =
                                    writable {
                                        val testCase = rpcEventStreamTestCase.inner
                                        rust(
                                            """
                                            let initial_response = msg(
                                                "event",
                                                "initial-response",
                                                ${testCase.eventStreamMessageContentType.dq()},
                                                // Provide a bogus string when either a JSON string or the base64 encoding of a JSON string is expected
                                                ${testCase.generateRustPayloadInitializer("abc")},
                                            );
                                            let mut buffer = vec![];
                                            aws_smithy_eventstream::frame::write_message_to(&initial_response, &mut buffer).unwrap();
                                            let chunks: Vec<Result<bytes::Bytes, _>> = vec![Ok(buffer.into())];
                                            let initial_response_stream = futures_util::stream::iter(chunks);
                                            """,
                                        )
                                    },
                                eventStreamGenerator =
                                    writable {
                                        rust("let event_stream = initial_response_stream.chain(futures_util::stream::iter(chunks));")
                                    },
                                sendResultHandler =
                                    writable {
                                        rustTemplate(
                                            """
                                            match response {
                                                Err(e) => {
                                                    assert!(e
                                                        .into_source()
                                                        .unwrap()
                                                        .downcast_ref::<#{DeserializeError}>()
                                                        .is_some());
                                                }
                                                otherwise => {
                                                    panic!(
                                                        "Expected ResponseError with DeserializeError being its source, got: {otherwise:?}"
                                                    );
                                                }
                                            }
                                            """,
                                            "DeserializeError" to
                                                if (rpcEventStreamTestCase.inner.protocolShapeId == "smithy.protocols#rpcv2Cbor") {
                                                    RuntimeType.smithyCbor(codegenContext.runtimeConfig).resolve("decode::DeserializeError")
                                                } else {
                                                    RuntimeType.smithyJson(codegenContext.runtimeConfig).resolve("deserialize::error::DeserializeError")
                                                },
                                        )
                                    },
                                initialResponseAssertion = writable {},
                                eventStreamReceiver = writable {},
                            )
                        }
                    }
                    NonEventStreamMemberInOutput.OPTIONAL_UNSET -> {
                        tokioTest("without_initial_response") {
                            writeTestBody(
                                codegenContext,
                                initialResponseStreamGenerator = writable {},
                                eventStreamGenerator =
                                    writable {
                                        rust("let event_stream = futures_util::stream::iter(chunks);")
                                    },
                                initialResponseAssertion =
                                    writable {
                                        rust("assert!(response.test_string().is_none());")
                                    },
                            )
                        }
                    }
                    NonEventStreamMemberInOutput.NONE -> {
                        tokioTest("event_stream_member_only") {
                            writeTestBody(
                                codegenContext,
                                initialResponseStreamGenerator = writable {},
                                eventStreamGenerator =
                                    writable {
                                        rust("let event_stream = futures_util::stream::iter(chunks);")
                                    },
                                initialResponseAssertion = writable {},
                            )
                        }
                    }
                }
            }
        }
    }
}
