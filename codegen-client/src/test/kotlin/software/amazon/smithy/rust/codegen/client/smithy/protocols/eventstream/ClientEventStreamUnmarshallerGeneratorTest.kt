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
import software.amazon.smithy.model.shapes.ShapeId
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
import software.amazon.smithy.rust.codegen.core.util.isRpcBoundProtocol
import java.util.stream.Stream

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

    @ParameterizedTest
    @ArgumentsSource(RpcEventStreamTestCasesProvider::class)
    fun rpcEventStreamTest(rpcEventStreamTestCase: RpcEventStreamTestCase) {
        fun RustWriter.writeTestBody(
            codegenContext: CodegenContext,
            initialResponseStreamGenerator: Writable,
            eventStreamGenerator: Writable,
            initialResponseAssertion: Writable,
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
                let mut res = client
                    .test_stream_op()
                    .value(EventStreamSender::from(stream))
                    .send()
                    .await
                    .unwrap();

                #{assert_initial_response:W}

                if let Some(v) = res.value.recv().await.unwrap() {
                    match v {
                        TestStream::MessageWithString(message_with_string) => {
                            assert_eq!("hello, world!", message_with_string.data().unwrap())
                        }
                        _ => panic!("matched on unexpected variant"),
                    }
                } else {
                    panic!("should receive at least one frame");
                }
                """,
                "assert_initial_response" to initialResponseAssertion,
                "capture_request" to RuntimeType.captureRequest(codegenContext.runtimeConfig),
                "event_stream" to eventStreamGenerator,
                "futures_util" to CargoDependency.FuturesUtil.toType(),
                "initial_response_stream" to initialResponseStreamGenerator,
            )
        }

        clientIntegrationTest(
            rpcEventStreamTestCase.inner.model,
            IntegrationTestParams(service = "test#TestService", addModuleToEventStreamAllowList = true),
        ) { codegenContext, rustCrate ->
            rustCrate.testModule {
                rust("##![allow(unused_imports, dead_code)]")
                writeUnmarshallTestUtil(codegenContext)

                if (rpcEventStreamTestCase.nonEventStreamMember != NonEventStreamMemberInOutput.NONE) {
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
                                    rust("assert_eq!(${rpcEventStreamTestCase.expectedValueInInitialResponse}, res.test_string());")
                                },
                        )
                    }
                }

                if (rpcEventStreamTestCase.nonEventStreamMember == NonEventStreamMemberInOutput.OPTIONAL) {
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
                                    rust("assert!(res.test_string().is_none());")
                                },
                        )
                    }
                }

                if (rpcEventStreamTestCase.nonEventStreamMember == NonEventStreamMemberInOutput.NONE) {
                    tokioTest("without_initial_response") {
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

private class RpcEventStreamTestCasesProvider : ArgumentsProvider {
    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        EventStreamTestModels.TEST_CASES.filter { testCase: EventStreamTestModels.TestCase ->
            ShapeId.from(testCase.protocolShapeId)?.isRpcBoundProtocol ?: false
        }.flatMap { testCase: EventStreamTestModels.TestCase ->
            val key = "testString"
            val value = "this is test"
            val payload =
                if (testCase.protocolShapeId == "smithy.protocols#rpcv2Cbor") {
                    EventStreamTestModels.base64EncodeJson("""{"$key":"$value"}""")
                } else {
                    """{"$key":"$value"}"""
                }
            listOf(
                RpcEventStreamTestCase(
                    inner = testCase,
                    nonEventStreamMember = NonEventStreamMemberInOutput.NONE,
                    // doesn't matter since it's unreachable
                    expectedValueInInitialResponse = "",
                ),
                RpcEventStreamTestCase(
                    inner =
                        testCase.withNonEventStreamMembers("""@httpHeader("X-Test-String")$key: String,""")
                            .copy(eventStreamInitialResponsePayload = payload),
                    nonEventStreamMember = NonEventStreamMemberInOutput.OPTIONAL,
                    expectedValueInInitialResponse = "Some(${value.dq()})",
                ),
                RpcEventStreamTestCase(
                    inner =
                        testCase.withNonEventStreamMembers("""@required@httpHeader("X-Test-String")$key: String,""")
                            .copy(eventStreamInitialResponsePayload = payload),
                    nonEventStreamMember = NonEventStreamMemberInOutput.REQUIRED,
                    expectedValueInInitialResponse = value.dq(),
                ),
            )
        }.map { Arguments.of(it) }.stream()
}

enum class NonEventStreamMemberInOutput {
    OPTIONAL,
    REQUIRED,
    NONE,
}

data class RpcEventStreamTestCase(
    val inner: EventStreamTestModels.TestCase,
    val nonEventStreamMember: NonEventStreamMemberInOutput,
    val expectedValueInInitialResponse: String,
)
