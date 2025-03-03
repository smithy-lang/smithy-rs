/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import org.intellij.lang.annotations.Language
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.util.lookup
import java.util.Base64

object EventStreamUnmarshallTestCases {
    fun RustWriter.writeUnmarshallTestUtil(codegenContext: CodegenContext) {
        val testStreamError =
            codegenContext.symbolProvider.symbolForEventStreamError(codegenContext.model.lookup("test#TestStream"))
        val typesModule = codegenContext.symbolProvider.moduleForShape(codegenContext.model.lookup("test#TestStruct"))

        rust(
            """
            use aws_smithy_eventstream::frame::{UnmarshallMessage, UnmarshalledMessage};
            use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
            use aws_smithy_types::{Blob, DateTime};
            use $testStreamError;
            use ${typesModule.fullyQualifiedPath()}::*;

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
    }

    fun RustWriter.writeUnmarshallTestCases(
        codegenContext: CodegenContext,
        testCase: EventStreamTestModels.TestCase,
        optionalBuilderInputs: Boolean = false,
    ) {
        val generator = "crate::event_stream_serde::TestStreamUnmarshaller"
        writeUnmarshallTestUtil(codegenContext)

        unitTest("message_with_blob") {
            rustTemplate(
                """
                let message = msg("event", "MessageWithBlob", "application/octet-stream", b"hello, world!");
                let result = $generator::new().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                assert_eq!(
                    TestStream::MessageWithBlob(
                        MessageWithBlob::builder().data(#{DataInput:W}).build()
                    ),
                    expect_event(result.unwrap())
                );
                """,
                "DataInput" to
                    conditionalBuilderInput(
                        """
                        Blob::new(&b"hello, world!"[..])
                        """,
                        conditional = optionalBuilderInputs,
                    ),
            )
        }

        unitTest("message_with_string") {
            rustTemplate(
                """
                let message = msg("event", "MessageWithString", "text/plain", b"hello, world!");
                let result = $generator::new().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                assert_eq!(
                    TestStream::MessageWithString(MessageWithString::builder().data(#{DataInput}).build()),
                    expect_event(result.unwrap())
                );
                """,
                "DataInput" to conditionalBuilderInput("\"hello, world!\"", conditional = optionalBuilderInputs),
            )
        }

        unitTest("message_with_struct") {
            rustTemplate(
                """
                let message = msg(
                    "event",
                    "MessageWithStruct",
                    "${testCase.responseContentType}",
                    ${testCase.generateRustPayloadInitializer(testCase.validTestStruct)}
                );
                let result = $generator::new().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                assert_eq!(
                    TestStream::MessageWithStruct(MessageWithStruct::builder().some_struct(#{StructInput}).build()),
                    expect_event(result.unwrap())
                );
                """,
                "StructInput" to
                    conditionalBuilderInput(
                        """
                        TestStruct::builder()
                            .some_string(#{StringInput})
                            .some_int(#{IntInput})
                            .build()
                        """,
                        conditional = optionalBuilderInputs,
                        "StringInput" to conditionalBuilderInput("\"hello\"", conditional = optionalBuilderInputs),
                        "IntInput" to conditionalBuilderInput("5", conditional = optionalBuilderInputs),
                    ),
            )
        }

        unitTest("message_with_union") {
            rustTemplate(
                """
                let message = msg(
                    "event",
                    "MessageWithUnion",
                    "${testCase.responseContentType}",
                    ${testCase.generateRustPayloadInitializer(testCase.validTestUnion)}
                );
                let result = $generator::new().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                assert_eq!(
                    TestStream::MessageWithUnion(MessageWithUnion::builder().some_union(#{UnionInput}).build()),
                    expect_event(result.unwrap())
                );
                """,
                "UnionInput" to conditionalBuilderInput("TestUnion::Foo(\"hello\".into())", conditional = optionalBuilderInputs),
            )
        }

        unitTest("message_with_headers") {
            rustTemplate(
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
                let result = $generator::new().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                assert_eq!(
                    TestStream::MessageWithHeaders(MessageWithHeaders::builder()
                        .blob(#{BlobInput})
                        .boolean(#{BoolInput})
                        .byte(#{ByteInput})
                        .int(#{IntInput})
                        .long(#{LongInput})
                        .short(#{ShortInput})
                        .string(#{StringInput})
                        .timestamp(#{TimestampInput})
                        .build()
                    ),
                    expect_event(result.unwrap())
                );
                """,
                "BlobInput" to conditionalBuilderInput("Blob::new(&b\"test\"[..])", conditional = optionalBuilderInputs),
                "BoolInput" to conditionalBuilderInput("true", conditional = optionalBuilderInputs),
                "ByteInput" to conditionalBuilderInput("55i8", conditional = optionalBuilderInputs),
                "IntInput" to conditionalBuilderInput("100_000i32", conditional = optionalBuilderInputs),
                "LongInput" to conditionalBuilderInput("9_000_000_000i64", conditional = optionalBuilderInputs),
                "ShortInput" to conditionalBuilderInput("16_000i16", conditional = optionalBuilderInputs),
                "StringInput" to conditionalBuilderInput("\"test\"", conditional = optionalBuilderInputs),
                "TimestampInput" to conditionalBuilderInput("DateTime::from_secs(5)", conditional = optionalBuilderInputs),
            )
        }

        unitTest("message_with_header_and_payload") {
            rustTemplate(
                """
                let message = msg("event", "MessageWithHeaderAndPayload", "application/octet-stream", b"payload")
                    .add_header(Header::new("header", HeaderValue::String("header".into())));
                let result = $generator::new().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                assert_eq!(
                    TestStream::MessageWithHeaderAndPayload(MessageWithHeaderAndPayload::builder()
                        .header(#{HeaderInput})
                        .payload(#{PayloadInput})
                        .build()
                    ),
                    expect_event(result.unwrap())
                );
                """,
                "HeaderInput" to conditionalBuilderInput("\"header\"", conditional = optionalBuilderInputs),
                "PayloadInput" to conditionalBuilderInput("Blob::new(&b\"payload\"[..])", conditional = optionalBuilderInputs),
            )
        }

        unitTest("message_with_no_header_payload_traits") {
            rustTemplate(
                """
                let message = msg(
                    "event",
                    "MessageWithNoHeaderPayloadTraits",
                    "${testCase.responseContentType}",
                    ${testCase.generateRustPayloadInitializer(testCase.validMessageWithNoHeaderPayloadTraits)}
                );
                let result = $generator::new().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                assert_eq!(
                    TestStream::MessageWithNoHeaderPayloadTraits(MessageWithNoHeaderPayloadTraits::builder()
                        .some_int(#{IntInput})
                        .some_string(#{StringInput})
                        .build()
                    ),
                    expect_event(result.unwrap())
                );
                """,
                "IntInput" to conditionalBuilderInput("5", conditional = optionalBuilderInputs),
                "StringInput" to conditionalBuilderInput("\"hello\"", conditional = optionalBuilderInputs),
            )
        }

        unitTest("some_error") {
            rustTemplate(
                """
                let message = msg(
                    "exception",
                    "SomeError",
                    "${testCase.responseContentType}",
                    ${testCase.generateRustPayloadInitializer(testCase.validSomeError)}
                );
                let result = $generator::new().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                match expect_error(result.unwrap()) {
                    TestStreamError::SomeError(err) => assert_eq!(Some("some error"), err.message()),
                    #{AllowUnreachablePatterns:W}
                    kind => panic!("expected SomeError, but got {:?}", kind),
                }
                """,
                "AllowUnreachablePatterns" to writable { Attribute.AllowUnreachablePatterns.render(this) },
            )
        }

        unitTest(
            "bad_content_type",
            """
            let message = msg(
                "event",
                "MessageWithBlob",
                "wrong-content-type",
                ${testCase.generateRustPayloadInitializer(testCase.validTestStruct)}
            );
            let result = $generator::new().unmarshall(&message);
            assert!(result.is_err(), "expected error, got: {:?}", result);
            assert!(format!("{}", result.err().unwrap()).contains("expected :content-type to be"));
            """,
        )
    }

    /**
     * Generates a Rust-compatible initializer string for a given payload.
     *
     * This function handles two different scenarios based on the event stream message content type:
     *
     * 1. For CBOR payloads (content type "application/cbor"):
     *    - The input payload is expected to be a base64 encoded CBOR value.
     *    - It decodes the base64 string and generates a Rust byte array initializer.
     *    - The output format is: &[0xFFu8, 0xFFu8, ...] where FF are hexadecimal values.
     *
     * 2. For all other content types:
     *    - It returns a Rust raw string literal initializer.
     *    - The output format is: br##"original_payload"##
     */
    fun EventStreamTestModels.TestCase.generateRustPayloadInitializer(payload: String): String {
        return if (this.eventStreamMessageContentType == "application/cbor") {
            Base64.getDecoder().decode(payload)
                .joinToString(
                    prefix = "&[",
                    postfix = "]",
                    transform = { "0x${it.toUByte().toString(16).padStart(2, '0')}u8" },
                )
        } else {
            """
            br##"$payload"##
            """
        }
    }
}

internal fun conditionalBuilderInput(
    @Language("Rust", prefix = "macro_rules! foo { () =>  {{\n", suffix = "\n}}}") contents: String,
    conditional: Boolean,
    vararg ctx: Pair<String, Any>,
): Writable =
    writable {
        conditionalBlock("Some(", ".into())", conditional = conditional) {
            rustTemplate(contents, *ctx)
        }
    }
