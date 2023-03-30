/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

internal object EventStreamUnmarshallTestCases {
    internal fun RustWriter.writeUnmarshallTestCases(
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
            CodegenTarget.CLIENT -> "TestStreamErrorKind::SomeError" to ".kind"
            CodegenTarget.SERVER -> "TestStreamError::SomeError" to ""
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
}
