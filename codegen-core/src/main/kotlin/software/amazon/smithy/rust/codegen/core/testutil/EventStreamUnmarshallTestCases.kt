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
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget

object EventStreamUnmarshallTestCases {
    fun RustWriter.writeUnmarshallTestCases(
        testCase: EventStreamTestModels.TestCase,
        codegenTarget: CodegenTarget,
        generator: String,
        codegenContext: CodegenContext,
    ) {
        val optionalInputs = codegenTarget == CodegenTarget.SERVER
        val crateName = codegenContext.moduleUseName()

        rust(
            """
            use aws_smithy_eventstream::frame::{Header, HeaderValue, Message, UnmarshallMessage, UnmarshalledMessage};
            use aws_smithy_types::{Blob, DateTime};
            use $crateName::error::*;
            use $crateName::model::*;

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
                "DataInput" to conditionalBuilderInput(
                    """
                    Blob::new(&b"hello, world!"[..])
                    """,
                    conditional = optionalInputs,
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
                "DataInput" to conditionalBuilderInput("\"hello, world!\".into()", conditional = optionalInputs),
            )
        }

        unitTest("message_with_struct") {
            rustTemplate(
                """
                let message = msg(
                    "event",
                    "MessageWithStruct",
                    "${testCase.responseContentType}",
                    br##"${testCase.validTestStruct}"##
                );
                let result = $generator::new().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                assert_eq!(
                    TestStream::MessageWithStruct(MessageWithStruct::builder().some_struct(#{StructInput}).build()),
                    expect_event(result.unwrap())
                );
                """,
                "StructInput" to conditionalBuilderInput(
                    """
                    TestStruct::builder()
                        .some_string(#{StringInput})
                        .some_int(#{IntInput})
                        .build()
                    """,
                    conditional = optionalInputs,
                    "StringInput" to conditionalBuilderInput("\"hello\".into()", conditional = optionalInputs),
                    "IntInput" to conditionalBuilderInput("5", conditional = optionalInputs),
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
                    br##"${testCase.validTestUnion}"##
                );
                let result = $generator::new().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                assert_eq!(
                    TestStream::MessageWithUnion(MessageWithUnion::builder().some_union(#{UnionInput}).build()),
                    expect_event(result.unwrap())
                );
                """,
                "UnionInput" to conditionalBuilderInput("TestUnion::Foo(\"hello\".into())", conditional = optionalInputs),
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
                "BlobInput" to conditionalBuilderInput("Blob::new(&b\"test\"[..])", conditional = optionalInputs),
                "BoolInput" to conditionalBuilderInput("true", conditional = optionalInputs),
                "ByteInput" to conditionalBuilderInput("55i8", conditional = optionalInputs),
                "IntInput" to conditionalBuilderInput("100_000i32", conditional = optionalInputs),
                "LongInput" to conditionalBuilderInput("9_000_000_000i64", conditional = optionalInputs),
                "ShortInput" to conditionalBuilderInput("16_000i16", conditional = optionalInputs),
                "StringInput" to conditionalBuilderInput("\"test\".into()", conditional = optionalInputs),
                "TimestampInput" to conditionalBuilderInput("DateTime::from_secs(5)", conditional = optionalInputs),
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
                "HeaderInput" to conditionalBuilderInput("\"header\".into()", conditional = optionalInputs),
                "PayloadInput" to conditionalBuilderInput("Blob::new(&b\"payload\"[..])", conditional = optionalInputs),
            )
        }

        unitTest("message_with_no_header_payload_traits") {
            rustTemplate(
                """
                let message = msg(
                    "event",
                    "MessageWithNoHeaderPayloadTraits",
                    "${testCase.responseContentType}",
                    br##"${testCase.validMessageWithNoHeaderPayloadTraits}"##
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
                "IntInput" to conditionalBuilderInput("5", conditional = optionalInputs),
                "StringInput" to conditionalBuilderInput("\"hello\".into()", conditional = optionalInputs),
            )
        }

        val (someError, kindSuffix) = when (codegenTarget) {
            CodegenTarget.CLIENT -> "TestStreamErrorKind::SomeError" to ".kind"
            CodegenTarget.SERVER -> "TestStreamError::SomeError" to ""
        }

        unitTest("some_error") {
            rustTemplate(
                """
                let message = msg(
                    "exception",
                    "SomeError",
                    "${testCase.responseContentType}",
                    br##"${testCase.validSomeError}"##
                );
                let result = $generator::new().unmarshall(&message);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                match expect_error(result.unwrap())$kindSuffix {
                    $someError(err) => assert_eq!(Some("some error"), err.message()),

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
                br#"${testCase.validTestStruct}"#
            );
            let result = $generator::new().unmarshall(&message);
            assert!(result.is_err(), "expected error, got: {:?}", result);
            assert!(format!("{}", result.err().unwrap()).contains("expected :content-type to be"));
            """,
        )
    }
}

fun conditionalBuilderInput(
    @Language("Rust", prefix = "macro_rules! foo { () =>  {{\n", suffix = "\n}}}") contents: String,
    conditional: Boolean,
    vararg ctx: Pair<String, Any>,
): Writable =
    writable {
        conditionalBlock("Some(", ")", conditional = conditional) {
            rustTemplate(contents, *ctx)
        }
    }
