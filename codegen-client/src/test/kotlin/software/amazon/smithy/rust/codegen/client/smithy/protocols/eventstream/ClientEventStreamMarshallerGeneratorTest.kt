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
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamMarshallTestCases.writeMarshallTestCases
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamUnmarshallTestCases.generateRustPayloadInitializer
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.lookup
import java.util.stream.Stream

class ClientEventStreamMarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(TestCasesProvider::class)
    fun test(testCase: EventStreamTestModels.TestCase) {
        clientIntegrationTest(testCase.model) { codegenContext, rustCrate ->
            rustCrate.testModule {
                writeMarshallTestCases(codegenContext, testCase, optionalBuilderInputs = false)
            }
        }
    }

    @ParameterizedTest
    @ArgumentsSource(RpcEventStreamTestCasesProvider::class)
    fun rpcEventStreamTest(rpcEventStreamTestCase: RpcEventStreamTestCase) {
        fun RustWriter.writeTestBody(
            codegenContext: CodegenContext,
            initialRequestAssertion: Writable,
            setInput: Writable,
        ) {
            rustTemplate(
                """
                use aws_smithy_http::event_stream::EventStreamSender;
                use aws_smithy_types::body::SdkBody;
                use #{futures_util}::StreamExt;

                let (http_client, rx) = #{capture_request}(None);
                let conf = crate::Config::builder()
                    .endpoint_url("http://localhost:1234")
                    .http_client(http_client.clone())
                    .behavior_version_latest()
                    .build();
                let client = crate::Client::from_conf(conf);

                let event = TestStream::MessageWithString(
                    MessageWithString::builder().data("hello, world!").build(),
                );
                let stream = #{futures_util}::stream::iter(vec![Ok(event)]);
                let _ = client
                    .test_stream_op()
                    #{set_input:W}
                    .value(EventStreamSender::from(stream))
                    .send()
                    .await
                    .unwrap();

                let mut request = rx.expect_request();

                let mut body = ::aws_smithy_types::body::SdkBody::taken();
                std::mem::swap(&mut body, request.body_mut());

                let unmarshaller = crate::event_stream_serde::TestStreamUnmarshaller::new();
                let mut event_receiver = crate::event_receiver::EventReceiver::new(
                    ::aws_smithy_http::event_stream::Receiver::new(unmarshaller, body)
                );

                #{assert_initial_request:W}

                if let Some(event) = event_receiver.recv().await.unwrap() {
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
                "assert_initial_request" to initialRequestAssertion,
                "capture_request" to RuntimeType.captureRequest(codegenContext.runtimeConfig),
                "futures_util" to CargoDependency.FuturesUtil.toType(),
                "set_input" to setInput,
            )
        }

        val testCase = rpcEventStreamTestCase.inner
        clientIntegrationTest(
            testCase.model,
            IntegrationTestParams(service = "test#TestService"),
        ) { codegenContext, rustCrate ->
            rustCrate.testModule {
                rust("##![allow(unused_imports, dead_code)]")

                val typesModule =
                    codegenContext.symbolProvider.moduleForShape(codegenContext.model.lookup("test#TestStruct"))
                rust(
                    """
                    use aws_smithy_eventstream::frame::{UnmarshallMessage, UnmarshalledMessage};
                    use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
                    use aws_smithy_types::{Blob, DateTime};
                    use ${typesModule.fullyQualifiedPath()}::*;
                    """,
                )

                if (rpcEventStreamTestCase.nonEventStreamMember == NonEventStreamMemberInOutput.NONE) {
                    tokioTest("without_initial_request") {
                        writeTestBody(
                            codegenContext,
                            initialRequestAssertion =
                                writable {
                                    rust("assert!(event_receiver.try_recv_initial_request().await.unwrap().is_none());")
                                },
                            setInput = writable {},
                        )
                    }
                } else {
                    tokioTest("with_initial_request") {
                        writeTestBody(
                            codegenContext,
                            initialRequestAssertion =
                                writable {
                                    rust(
                                        """
                                        let msg = event_receiver
                                            .try_recv_initial_request()
                                            .await
                                            .unwrap()
                                            .expect("should receive initial-request");
                                        assert!(msg
                                            .headers()
                                            .iter()
                                            .find(|p| p.value().as_string().unwrap().as_str() == "initial-request")
                                            .is_some());
                                        assert_eq!(
                                            msg.payload(),
                                            &bytes::Bytes::from_static(${testCase.generateRustPayloadInitializer(rpcEventStreamTestCase.expectedInInitialRequest)})
                                        );
                                        """,
                                    )
                                },
                            setInput =
                                if (rpcEventStreamTestCase.nonEventStreamMember == NonEventStreamMemberInOutput.OPTIONAL_UNSET) {
                                    writable {}
                                } else {
                                    writable {
                                        rust(""".test_string("this is test")""")
                                    }
                                },
                        )
                    }
                }
            }
        }
    }
}

class TestCasesProvider : ArgumentsProvider {
    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        EventStreamTestModels.TEST_CASES.map { Arguments.of(it) }.stream()
}

/**
 * Enum indicating the presence and state of a non-event stream member in [EventStreamTestModels] in `TestStreamInputOutput`.
 *
 * * `OPTIONAL_SET`: The non-stream member is optional and has a value set (input setter is called for input, or response payload is provided for output).
 * * `OPTIONAL_UNSET`: The non-stream member is optional but has no value set (no input setter is called for input, and no response payload is included for output).
 * * `REQUIRED`: The non-stream member is required and must be present.
 * * `NONE`: The non-stream member does not exist in `TestStreamInputOutput`.
 */
enum class NonEventStreamMemberInOutput {
    OPTIONAL_SET,
    OPTIONAL_UNSET,
    REQUIRED,
    NONE,
}

data class RpcEventStreamTestCase(
    val inner: EventStreamTestModels.TestCase,
    val nonEventStreamMember: NonEventStreamMemberInOutput,
    val expectedInInitialRequest: String = "",
    val expectedInInitialResponse: String = "",
)

class RpcEventStreamTestCasesProvider : ArgumentsProvider {
    private val rpcBoundProtocols =
        setOf(
            "awsJson1_0",
            "awsJson1_1",
            "awsQuery",
            "ec2Query",
            "rpcv2Cbor",
        )

    private val ShapeId.isRpcBoundProtocol
        get() = rpcBoundProtocols.contains(name)

    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        EventStreamTestModels.TEST_CASES.filter { testCase: EventStreamTestModels.TestCase ->
            ShapeId.from(testCase.protocolShapeId)?.isRpcBoundProtocol ?: false
        }.flatMap { testCase: EventStreamTestModels.TestCase ->
            val key = "testString"
            val value = "this is test"

            fun payload(jsonString: String): String =
                if (testCase.protocolShapeId == "smithy.protocols#rpcv2Cbor") {
                    EventStreamTestModels.base64EncodeJson(jsonString)
                } else {
                    jsonString
                }
            listOf(
                RpcEventStreamTestCase(
                    inner = testCase,
                    nonEventStreamMember = NonEventStreamMemberInOutput.NONE,
                ),
                RpcEventStreamTestCase(
                    inner =
                        testCase.withNonEventStreamMembers("""@httpHeader("X-Test-String")$key: String,"""),
                    nonEventStreamMember = NonEventStreamMemberInOutput.OPTIONAL_UNSET,
                    expectedInInitialRequest = payload("{}"),
                ),
                RpcEventStreamTestCase(
                    inner =
                        testCase.withNonEventStreamMembers("""@httpHeader("X-Test-String")$key: String,""")
                            .copy(eventStreamInitialResponsePayload = payload("""{"$key":"$value"}""")),
                    nonEventStreamMember = NonEventStreamMemberInOutput.OPTIONAL_SET,
                    expectedInInitialRequest = payload("""{"$key":"$value"}"""),
                    expectedInInitialResponse = "Some(${value.dq()})",
                ),
                RpcEventStreamTestCase(
                    inner =
                        testCase.withNonEventStreamMembers("""@required@httpHeader("X-Test-String")$key: String,""")
                            .copy(eventStreamInitialResponsePayload = payload("""{"$key":"$value"}""")),
                    nonEventStreamMember = NonEventStreamMemberInOutput.REQUIRED,
                    expectedInInitialRequest = payload("""{"$key":"$value"}"""),
                    expectedInInitialResponse = value.dq(),
                ),
            )
        }.map { Arguments.of(it) }.stream()
}
