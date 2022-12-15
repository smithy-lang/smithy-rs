/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.ErrorsModule
import software.amazon.smithy.rust.codegen.core.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.CombinedErrorGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ServerCombinedErrorGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsQueryProtocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Ec2QueryProtocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestXml
import software.amazon.smithy.rust.codegen.core.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.outputShape
import kotlin.streams.toList

private fun fillInBaseModel(
    protocolName: String,
    extraServiceAnnotations: String = "",
): String = """
    namespace test

    use aws.protocols#$protocolName

    union TestUnion {
        Foo: String,
        Bar: Integer,
    }
    structure TestStruct {
        someString: String,
        someInt: Integer,
    }

    @error("client")
    structure SomeError {
        Message: String,
    }

    structure MessageWithBlob { @eventPayload data: Blob }
    structure MessageWithString { @eventPayload data: String }
    structure MessageWithStruct { @eventPayload someStruct: TestStruct }
    structure MessageWithUnion { @eventPayload someUnion: TestUnion }
    structure MessageWithHeaders {
        @eventHeader blob: Blob,
        @eventHeader boolean: Boolean,
        @eventHeader byte: Byte,
        @eventHeader int: Integer,
        @eventHeader long: Long,
        @eventHeader short: Short,
        @eventHeader string: String,
        @eventHeader timestamp: Timestamp,
    }
    structure MessageWithHeaderAndPayload {
        @eventHeader header: String,
        @eventPayload payload: Blob,
    }
    structure MessageWithNoHeaderPayloadTraits {
        someInt: Integer,
        someString: String,
    }

    @streaming
    union TestStream {
        MessageWithBlob: MessageWithBlob,
        MessageWithString: MessageWithString,
        MessageWithStruct: MessageWithStruct,
        MessageWithUnion: MessageWithUnion,
        MessageWithHeaders: MessageWithHeaders,
        MessageWithHeaderAndPayload: MessageWithHeaderAndPayload,
        MessageWithNoHeaderPayloadTraits: MessageWithNoHeaderPayloadTraits,
        SomeError: SomeError,
    }
    structure TestStreamInputOutput { @httpPayload @required value: TestStream }
    operation TestStreamOp {
        input: TestStreamInputOutput,
        output: TestStreamInputOutput,
        errors: [SomeError],
    }
    $extraServiceAnnotations
    @$protocolName
    service TestService { version: "123", operations: [TestStreamOp] }
"""

object EventStreamTestModels {
    private fun restJson1(): Model = fillInBaseModel("restJson1").asSmithyModel()
    private fun restXml(): Model = fillInBaseModel("restXml").asSmithyModel()
    private fun awsJson11(): Model = fillInBaseModel("awsJson1_1").asSmithyModel()
    private fun awsQuery(): Model =
        fillInBaseModel("awsQuery", "@xmlNamespace(uri: \"https://example.com\")").asSmithyModel()
    private fun ec2Query(): Model =
        fillInBaseModel("ec2Query", "@xmlNamespace(uri: \"https://example.com\")").asSmithyModel()

    data class TestCase(
        val protocolShapeId: String,
        val model: Model,
        val requestContentType: String,
        val responseContentType: String,
        val validTestStruct: String,
        val validMessageWithNoHeaderPayloadTraits: String,
        val validTestUnion: String,
        val validSomeError: String,
        val validUnmodeledError: String,
        val protocolBuilder: (CodegenContext) -> Protocol,
    ) {
        override fun toString(): String = protocolShapeId
    }

    val TEST_CASES = listOf(
        //
        // restJson1
        //
        TestCase(
            protocolShapeId = "aws.protocols#restJson1",
            model = restJson1(),
            requestContentType = "application/json",
            responseContentType = "application/json",
            validTestStruct = """{"someString":"hello","someInt":5}""",
            validMessageWithNoHeaderPayloadTraits = """{"someString":"hello","someInt":5}""",
            validTestUnion = """{"Foo":"hello"}""",
            validSomeError = """{"Message":"some error"}""",
            validUnmodeledError = """{"Message":"unmodeled error"}""",
        ) { RestJson(it) },

        //
        // awsJson1_1
        //
        TestCase(
            protocolShapeId = "aws.protocols#awsJson1_1",
            model = awsJson11(),
            requestContentType = "application/x-amz-json-1.1",
            responseContentType = "application/x-amz-json-1.1",
            validTestStruct = """{"someString":"hello","someInt":5}""",
            validMessageWithNoHeaderPayloadTraits = """{"someString":"hello","someInt":5}""",
            validTestUnion = """{"Foo":"hello"}""",
            validSomeError = """{"Message":"some error"}""",
            validUnmodeledError = """{"Message":"unmodeled error"}""",
        ) { AwsJson(it, AwsJsonVersion.Json11) },

        //
        // restXml
        //
        TestCase(
            protocolShapeId = "aws.protocols#restXml",
            model = restXml(),
            requestContentType = "application/xml",
            responseContentType = "application/xml",
            validTestStruct = """
                <TestStruct>
                    <someString>hello</someString>
                    <someInt>5</someInt>
                </TestStruct>
            """.trimIndent(),
            validMessageWithNoHeaderPayloadTraits = """
                <MessageWithNoHeaderPayloadTraits>
                    <someString>hello</someString>
                    <someInt>5</someInt>
                </MessageWithNoHeaderPayloadTraits>
            """.trimIndent(),
            validTestUnion = "<TestUnion><Foo>hello</Foo></TestUnion>",
            validSomeError = """
                <ErrorResponse>
                    <Error>
                        <Type>SomeError</Type>
                        <Code>SomeError</Code>
                        <Message>some error</Message>
                    </Error>
                </ErrorResponse>
            """.trimIndent(),
            validUnmodeledError = """
                <ErrorResponse>
                    <Error>
                        <Type>UnmodeledError</Type>
                        <Code>UnmodeledError</Code>
                        <Message>unmodeled error</Message>
                    </Error>
                </ErrorResponse>
            """.trimIndent(),
        ) { RestXml(it) },

        //
        // awsQuery
        //
        TestCase(
            protocolShapeId = "aws.protocols#awsQuery",
            model = awsQuery(),
            requestContentType = "application/x-www-form-urlencoded",
            responseContentType = "text/xml",
            validTestStruct = """
                <TestStruct>
                    <someString>hello</someString>
                    <someInt>5</someInt>
                </TestStruct>
            """.trimIndent(),
            validMessageWithNoHeaderPayloadTraits = """
                <MessageWithNoHeaderPayloadTraits>
                    <someString>hello</someString>
                    <someInt>5</someInt>
                </MessageWithNoHeaderPayloadTraits>
            """.trimIndent(),
            validTestUnion = "<TestUnion><Foo>hello</Foo></TestUnion>",
            validSomeError = """
                <ErrorResponse>
                    <Error>
                        <Type>SomeError</Type>
                        <Code>SomeError</Code>
                        <Message>some error</Message>
                    </Error>
                </ErrorResponse>
            """.trimIndent(),
            validUnmodeledError = """
                <ErrorResponse>
                    <Error>
                        <Type>UnmodeledError</Type>
                        <Code>UnmodeledError</Code>
                        <Message>unmodeled error</Message>
                    </Error>
                </ErrorResponse>
            """.trimIndent(),
        ) { AwsQueryProtocol(it) },

        //
        // ec2Query
        //
        TestCase(
            protocolShapeId = "aws.protocols#ec2Query",
            model = ec2Query(),
            requestContentType = "application/x-www-form-urlencoded",
            responseContentType = "text/xml",
            validTestStruct = """
                <TestStruct>
                    <someString>hello</someString>
                    <someInt>5</someInt>
                </TestStruct>
            """.trimIndent(),
            validMessageWithNoHeaderPayloadTraits = """
                <MessageWithNoHeaderPayloadTraits>
                    <someString>hello</someString>
                    <someInt>5</someInt>
                </MessageWithNoHeaderPayloadTraits>
            """.trimIndent(),
            validTestUnion = "<TestUnion><Foo>hello</Foo></TestUnion>",
            validSomeError = """
                <Response>
                    <Errors>
                        <Error>
                            <Type>SomeError</Type>
                            <Code>SomeError</Code>
                            <Message>some error</Message>
                        </Error>
                    </Errors>
                </Response>
            """.trimIndent(),
            validUnmodeledError = """
                <Response>
                    <Errors>
                        <Error>
                            <Type>UnmodeledError</Type>
                            <Code>UnmodeledError</Code>
                            <Message>unmodeled error</Message>
                        </Error>
                    </Errors>
                </Response>
            """.trimIndent(),
        ) { Ec2QueryProtocol(it) },
    )
}

data class TestEventStreamProject(
    val model: Model,
    val serviceShape: ServiceShape,
    val operationShape: OperationShape,
    val streamShape: UnionShape,
    val symbolProvider: RustSymbolProvider,
    val project: TestWriterDelegator,
)

object EventStreamTestTools {
    fun runTestCase(
        testCase: EventStreamTestModels.TestCase,
        codegenTarget: CodegenTarget,
        createSymbolProvider: (Model) -> RustSymbolProvider,
        renderGenerator: (CodegenContext, TestEventStreamProject, Protocol) -> RuntimeType,
        marshall: Boolean,
    ) {
        val test = generateTestProject(testCase, codegenTarget, createSymbolProvider)

        val codegenContext = CodegenContext(
            test.model,
            test.symbolProvider,
            test.serviceShape,
            ShapeId.from(testCase.protocolShapeId),
            testRustSettings(),
            target = codegenTarget,
        )
        val protocol = testCase.protocolBuilder(codegenContext)
        val generator = renderGenerator(codegenContext, test, protocol)

        test.project.lib {
            if (marshall) {
                writeMarshallTestCases(testCase, generator)
            } else {
                writeUnmarshallTestCases(testCase, codegenTarget, generator)
            }
        }
        test.project.compileAndTest()
    }

    private fun generateTestProject(
        testCase: EventStreamTestModels.TestCase,
        codegenTarget: CodegenTarget,
        createSymbolProvider: (Model) -> RustSymbolProvider,
    ): TestEventStreamProject {
        val model = EventStreamNormalizer.transform(OperationNormalizer.transform(testCase.model))
        val serviceShape = model.expectShape(ShapeId.from("test#TestService")) as ServiceShape
        val operationShape = model.expectShape(ShapeId.from("test#TestStreamOp")) as OperationShape
        val unionShape = model.expectShape(ShapeId.from("test#TestStream")) as UnionShape

        val symbolProvider = createSymbolProvider(model)
        val project = TestWorkspace.testProject(symbolProvider)
        val operationSymbol = symbolProvider.toSymbol(operationShape)
        project.withModule(ErrorsModule) {
            val errors = model.shapes()
                .filter { shape -> shape.isStructureShape && shape.hasTrait<ErrorTrait>() }
                .map { it.asStructureShape().get() }
                .toList()
            when (codegenTarget) {
                CodegenTarget.CLIENT -> CombinedErrorGenerator(model, symbolProvider, operationSymbol, errors).render(this)
                CodegenTarget.SERVER -> ServerCombinedErrorGenerator(model, symbolProvider, operationSymbol, errors).render(this)
            }
            for (shape in model.shapes().filter { shape -> shape.isStructureShape && shape.hasTrait<ErrorTrait>() }) {
                StructureGenerator(model, symbolProvider, this, shape as StructureShape).render(codegenTarget)
                val builderGen = BuilderGenerator(model, symbolProvider, shape)
                builderGen.render(this)
                implBlock(shape, symbolProvider) {
                    builderGen.renderConvenienceMethod(this)
                }
            }
        }
        project.withModule(ModelsModule) {
            val inputOutput = model.lookup<StructureShape>("test#TestStreamInputOutput")
            recursivelyGenerateModels(model, symbolProvider, inputOutput, this, codegenTarget)
        }
        project.withModule(RustModule.Output) {
            operationShape.outputShape(model).renderWithModelBuilder(model, symbolProvider, this)
        }
        return TestEventStreamProject(model, serviceShape, operationShape, unionShape, symbolProvider, project)
    }

    private fun recursivelyGenerateModels(
        model: Model,
        symbolProvider: RustSymbolProvider,
        shape: Shape,
        writer: RustWriter,
        mode: CodegenTarget,
    ) {
        for (member in shape.members()) {
            val target = model.expectShape(member.target)
            if (target is StructureShape || target is UnionShape) {
                if (target is StructureShape) {
                    target.renderWithModelBuilder(model, symbolProvider, writer)
                } else if (target is UnionShape) {
                    UnionGenerator(model, symbolProvider, writer, target, renderUnknownVariant = mode.renderUnknownVariant()).render()
                }
                recursivelyGenerateModels(model, symbolProvider, target, writer, mode)
            }
        }
    }

    private fun RustWriter.writeUnmarshallTestCases(
        testCase: EventStreamTestModels.TestCase,
        codegenTarget: CodegenTarget,
        generator: RuntimeType,
    ) {
        rust(
            """
            use aws_smithy_eventstream::frame::{Header, HeaderValue, Message, UnmarshallMessage, UnmarshalledMessage};
            use aws_smithy_types::{Blob, DateTime};
            use crate::error::*;
            use crate::model::*;

            fn msg(
                message_type: &'static str,
                event_type: &'static str,
                content_type: &'static str,
                payload: &'static [u8],
            ) -> Message {
                let message = Message::new(payload)
                    .add_header(Header::new(":message-type", HeaderValue::String(message_type.into())))
                    .add_header(Header::new(":content-type", HeaderValue::String(content_type.into())));
                if message_type == "event" {
                    message.add_header(Header::new(":event-type", HeaderValue::String(event_type.into())))
                } else {
                    message.add_header(Header::new(":exception-type", HeaderValue::String(event_type.into())))
                }
            }
            fn expect_event<T: std::fmt::Debug, E: std::fmt::Debug>(unmarshalled: UnmarshalledMessage<T, E>) -> T {
                match unmarshalled {
                    UnmarshalledMessage::Event(event) => event,
                    _ => panic!("expected event, got: {:?}", unmarshalled),
                }
            }
            fn expect_error<T: std::fmt::Debug, E: std::fmt::Debug>(unmarshalled: UnmarshalledMessage<T, E>) -> E {
                match unmarshalled {
                    UnmarshalledMessage::Error(error) => error,
                    _ => panic!("expected error, got: {:?}", unmarshalled),
                }
            }
            """,
        )

        unitTest(
            name = "message_with_blob",
            test = """
                let message = msg("event", "MessageWithBlob", "application/octet-stream", b"hello, world!");
                let result = ${format(generator)}().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                assert_eq!(
                    TestStream::MessageWithBlob(
                        MessageWithBlob::builder().data(Blob::new(&b"hello, world!"[..])).build()
                    ),
                    expect_event(result.unwrap())
                );
            """,
        )

        if (codegenTarget == CodegenTarget.CLIENT) {
            unitTest(
                "unknown_message",
                """
                let message = msg("event", "NewUnmodeledMessageType", "application/octet-stream", b"hello, world!");
                let result = ${format(generator)}().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                assert_eq!(
                    TestStream::Unknown,
                    expect_event(result.unwrap())
                );
                """,
            )
        }

        unitTest(
            "message_with_string",
            """
            let message = msg("event", "MessageWithString", "text/plain", b"hello, world!");
            let result = ${format(generator)}().unmarshall(&message);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            assert_eq!(
                TestStream::MessageWithString(MessageWithString::builder().data("hello, world!").build()),
                expect_event(result.unwrap())
            );
            """,
        )

        unitTest(
            "message_with_struct",
            """
            let message = msg(
                "event",
                "MessageWithStruct",
                "${testCase.responseContentType}",
                br#"${testCase.validTestStruct}"#
            );
            let result = ${format(generator)}().unmarshall(&message);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            assert_eq!(
                TestStream::MessageWithStruct(MessageWithStruct::builder().some_struct(
                    TestStruct::builder()
                        .some_string("hello")
                        .some_int(5)
                        .build()
                ).build()),
                expect_event(result.unwrap())
            );
            """,
        )

        unitTest(
            "message_with_union",
            """
            let message = msg(
                "event",
                "MessageWithUnion",
                "${testCase.responseContentType}",
                br#"${testCase.validTestUnion}"#
            );
            let result = ${format(generator)}().unmarshall(&message);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            assert_eq!(
                TestStream::MessageWithUnion(MessageWithUnion::builder().some_union(
                    TestUnion::Foo("hello".into())
                ).build()),
                expect_event(result.unwrap())
            );
            """,
        )

        unitTest(
            "message_with_headers",
            """
            let message = msg("event", "MessageWithHeaders", "application/octet-stream", b"")
                .add_header(Header::new("blob", HeaderValue::ByteArray((&b"test"[..]).into())))
                .add_header(Header::new("boolean", HeaderValue::Bool(true)))
                .add_header(Header::new("byte", HeaderValue::Byte(55i8)))
                .add_header(Header::new("int", HeaderValue::Int32(100_000i32)))
                .add_header(Header::new("long", HeaderValue::Int64(9_000_000_000i64)))
                .add_header(Header::new("short", HeaderValue::Int16(16_000i16)))
                .add_header(Header::new("string", HeaderValue::String("test".into())))
                .add_header(Header::new("timestamp", HeaderValue::Timestamp(DateTime::from_secs(5))));
            let result = ${format(generator)}().unmarshall(&message);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            assert_eq!(
                TestStream::MessageWithHeaders(MessageWithHeaders::builder()
                    .blob(Blob::new(&b"test"[..]))
                    .boolean(true)
                    .byte(55i8)
                    .int(100_000i32)
                    .long(9_000_000_000i64)
                    .short(16_000i16)
                    .string("test")
                    .timestamp(DateTime::from_secs(5))
                    .build()
                ),
                expect_event(result.unwrap())
            );
            """,
        )

        unitTest(
            "message_with_header_and_payload",
            """
            let message = msg("event", "MessageWithHeaderAndPayload", "application/octet-stream", b"payload")
                .add_header(Header::new("header", HeaderValue::String("header".into())));
            let result = ${format(generator)}().unmarshall(&message);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            assert_eq!(
                TestStream::MessageWithHeaderAndPayload(MessageWithHeaderAndPayload::builder()
                    .header("header")
                    .payload(Blob::new(&b"payload"[..]))
                    .build()
                ),
                expect_event(result.unwrap())
            );
            """,
        )

        unitTest(
            "message_with_no_header_payload_traits",
            """
            let message = msg(
                "event",
                "MessageWithNoHeaderPayloadTraits",
                "${testCase.responseContentType}",
                br#"${testCase.validMessageWithNoHeaderPayloadTraits}"#
            );
            let result = ${format(generator)}().unmarshall(&message);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            assert_eq!(
                TestStream::MessageWithNoHeaderPayloadTraits(MessageWithNoHeaderPayloadTraits::builder()
                    .some_int(5)
                    .some_string("hello")
                    .build()
                ),
                expect_event(result.unwrap())
            );
            """,
        )

        val (someError, kindSuffix) = when (codegenTarget) {
            CodegenTarget.CLIENT -> listOf("TestStreamErrorKind::SomeError", ".kind")
            CodegenTarget.SERVER -> listOf("TestStreamError::SomeError", "")
        }
        unitTest(
            "some_error",
            """
            let message = msg(
                "exception",
                "SomeError",
                "${testCase.responseContentType}",
                br#"${testCase.validSomeError}"#
            );
            let result = ${format(generator)}().unmarshall(&message);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            match expect_error(result.unwrap())$kindSuffix {
                $someError(err) => assert_eq!(Some("some error"), err.message()),
                kind => panic!("expected SomeError, but got {:?}", kind),
            }
            """,
        )

        if (codegenTarget == CodegenTarget.CLIENT) {
            unitTest(
                "generic_error",
                """
                let message = msg(
                    "exception",
                    "UnmodeledError",
                    "${testCase.responseContentType}",
                    br#"${testCase.validUnmodeledError}"#
                );
                let result = ${format(generator)}().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                match expect_error(result.unwrap())$kindSuffix {
                    TestStreamErrorKind::Unhandled(err) => {
                        let message = format!("{}", aws_smithy_types::error::display::DisplayErrorContext(&err));
                        let expected = "message: \"unmodeled error\"";
                        assert!(message.contains(expected), "Expected '{message}' to contain '{expected}'");
                    }
                    kind => panic!("expected generic error, but got {:?}", kind),
                }
                """,
            )
        }

        unitTest(
            "bad_content_type",
            """
            let message = msg(
                "event",
                "MessageWithBlob",
                "wrong-content-type",
                br#"${testCase.validTestStruct}"#
            );
            let result = ${format(generator)}().unmarshall(&message);
            assert!(result.is_err(), "expected error, got: {:?}", result);
            assert!(format!("{}", result.err().unwrap()).contains("expected :content-type to be"));
            """,
        )
    }

    private fun RustWriter.writeMarshallTestCases(
        testCase: EventStreamTestModels.TestCase,
        generator: RuntimeType,
    ) {
        val protocolTestHelpers = CargoDependency.smithyProtocolTestHelpers(TestRuntimeConfig)
            .copy(scope = DependencyScope.Compile)
        rustTemplate(
            """
            use aws_smithy_eventstream::frame::{Message, Header, HeaderValue, MarshallMessage};
            use std::collections::HashMap;
            use aws_smithy_types::{Blob, DateTime};
            use crate::error::*;
            use crate::model::*;

            use #{validate_body};
            use #{MediaType};

            fn headers_to_map<'a>(headers: &'a [Header]) -> HashMap<String, &'a HeaderValue> {
                let mut map = HashMap::new();
                for header in headers {
                    map.insert(header.name().as_str().to_string(), header.value());
                }
                map
            }

            fn str_header(value: &'static str) -> HeaderValue {
                HeaderValue::String(value.into())
            }
            """,
            "validate_body" to protocolTestHelpers.toType().resolve("validate_body"),
            "MediaType" to protocolTestHelpers.toType().resolve("MediaType"),
        )

        unitTest(
            "message_with_blob",
            """
            let event = TestStream::MessageWithBlob(
                MessageWithBlob::builder().data(Blob::new(&b"hello, world!"[..])).build()
            );
            let result = ${format(generator)}().marshall(event);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            let message = result.unwrap();
            let headers = headers_to_map(message.headers());
            assert_eq!(&str_header("event"), *headers.get(":message-type").unwrap());
            assert_eq!(&str_header("MessageWithBlob"), *headers.get(":event-type").unwrap());
            assert_eq!(&str_header("application/octet-stream"), *headers.get(":content-type").unwrap());
            assert_eq!(&b"hello, world!"[..], message.payload());
            """,
        )

        unitTest(
            "message_with_string",
            """
            let event = TestStream::MessageWithString(
                MessageWithString::builder().data("hello, world!").build()
            );
            let result = ${format(generator)}().marshall(event);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            let message = result.unwrap();
            let headers = headers_to_map(message.headers());
            assert_eq!(&str_header("event"), *headers.get(":message-type").unwrap());
            assert_eq!(&str_header("MessageWithString"), *headers.get(":event-type").unwrap());
            assert_eq!(&str_header("text/plain"), *headers.get(":content-type").unwrap());
            assert_eq!(&b"hello, world!"[..], message.payload());
            """,
        )

        unitTest(
            "message_with_struct",
            """
            let event = TestStream::MessageWithStruct(
                MessageWithStruct::builder().some_struct(
                    TestStruct::builder()
                        .some_string("hello")
                        .some_int(5)
                        .build()
                ).build()
            );
            let result = ${format(generator)}().marshall(event);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            let message = result.unwrap();
            let headers = headers_to_map(message.headers());
            assert_eq!(&str_header("event"), *headers.get(":message-type").unwrap());
            assert_eq!(&str_header("MessageWithStruct"), *headers.get(":event-type").unwrap());
            assert_eq!(&str_header(${testCase.requestContentType.dq()}), *headers.get(":content-type").unwrap());

            validate_body(
                message.payload(),
                ${testCase.validTestStruct.dq()},
                MediaType::from(${testCase.requestContentType.dq()})
            ).unwrap();
            """,
        )

        unitTest(
            "message_with_union",
            """
            let event = TestStream::MessageWithUnion(MessageWithUnion::builder().some_union(
                TestUnion::Foo("hello".into())
            ).build());
            let result = ${format(generator)}().marshall(event);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            let message = result.unwrap();
            let headers = headers_to_map(message.headers());
            assert_eq!(&str_header("event"), *headers.get(":message-type").unwrap());
            assert_eq!(&str_header("MessageWithUnion"), *headers.get(":event-type").unwrap());
            assert_eq!(&str_header(${testCase.requestContentType.dq()}), *headers.get(":content-type").unwrap());

            validate_body(
                message.payload(),
                ${testCase.validTestUnion.dq()},
                MediaType::from(${testCase.requestContentType.dq()})
            ).unwrap();
            """,
        )

        unitTest(
            "message_with_headers",
            """
            let event = TestStream::MessageWithHeaders(MessageWithHeaders::builder()
                .blob(Blob::new(&b"test"[..]))
                .boolean(true)
                .byte(55i8)
                .int(100_000i32)
                .long(9_000_000_000i64)
                .short(16_000i16)
                .string("test")
                .timestamp(DateTime::from_secs(5))
                .build()
            );
            let result = ${format(generator)}().marshall(event);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            let actual_message = result.unwrap();
            let expected_message = Message::new(&b""[..])
                .add_header(Header::new(":message-type", HeaderValue::String("event".into())))
                .add_header(Header::new(":event-type", HeaderValue::String("MessageWithHeaders".into())))
                .add_header(Header::new("blob", HeaderValue::ByteArray((&b"test"[..]).into())))
                .add_header(Header::new("boolean", HeaderValue::Bool(true)))
                .add_header(Header::new("byte", HeaderValue::Byte(55i8)))
                .add_header(Header::new("int", HeaderValue::Int32(100_000i32)))
                .add_header(Header::new("long", HeaderValue::Int64(9_000_000_000i64)))
                .add_header(Header::new("short", HeaderValue::Int16(16_000i16)))
                .add_header(Header::new("string", HeaderValue::String("test".into())))
                .add_header(Header::new("timestamp", HeaderValue::Timestamp(DateTime::from_secs(5))));
            assert_eq!(expected_message, actual_message);
            """,
        )

        unitTest(
            "message_with_header_and_payload",
            """
            let event = TestStream::MessageWithHeaderAndPayload(MessageWithHeaderAndPayload::builder()
                .header("header")
                .payload(Blob::new(&b"payload"[..]))
                .build()
            );
            let result = ${format(generator)}().marshall(event);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            let actual_message = result.unwrap();
            let expected_message = Message::new(&b"payload"[..])
                .add_header(Header::new(":message-type", HeaderValue::String("event".into())))
                .add_header(Header::new(":event-type", HeaderValue::String("MessageWithHeaderAndPayload".into())))
                .add_header(Header::new("header", HeaderValue::String("header".into())))
                .add_header(Header::new(":content-type", HeaderValue::String("application/octet-stream".into())));
            assert_eq!(expected_message, actual_message);
            """,
        )

        unitTest(
            "message_with_no_header_payload_traits",
            """
            let event = TestStream::MessageWithNoHeaderPayloadTraits(MessageWithNoHeaderPayloadTraits::builder()
                .some_int(5)
                .some_string("hello")
                .build()
            );
            let result = ${format(generator)}().marshall(event);
            assert!(result.is_ok(), "expected ok, got: {:?}", result);
            let message = result.unwrap();
            let headers = headers_to_map(message.headers());
            assert_eq!(&str_header("event"), *headers.get(":message-type").unwrap());
            assert_eq!(&str_header("MessageWithNoHeaderPayloadTraits"), *headers.get(":event-type").unwrap());
            assert_eq!(&str_header(${testCase.requestContentType.dq()}), *headers.get(":content-type").unwrap());

            validate_body(
                message.payload(),
                ${testCase.validMessageWithNoHeaderPayloadTraits.dq()},
                MediaType::from(${testCase.requestContentType.dq()})
            ).unwrap();
            """,
        )
    }
}
