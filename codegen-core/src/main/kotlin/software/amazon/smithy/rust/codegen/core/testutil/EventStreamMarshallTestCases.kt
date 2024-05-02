/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import org.intellij.lang.annotations.Language
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.lookup

object EventStreamMarshallTestCases {
    fun RustWriter.writeMarshallTestCases(
        codegenContext: CodegenContext,
        testCase: EventStreamTestModels.TestCase,
        optionalBuilderInputs: Boolean,
    ) {
        val generator = "crate::event_stream_serde::TestStreamMarshaller"

        val protocolTestHelpers =
            CargoDependency.smithyProtocolTestHelpers(TestRuntimeConfig)
                .copy(scope = DependencyScope.Compile)

        fun builderInput(
            @Language("Rust", prefix = "macro_rules! foo { () =>  {{\n", suffix = "\n}}}")
            input: String,
            vararg ctx: Pair<String, Any>,
        ): Writable = conditionalBuilderInput(input, conditional = optionalBuilderInputs, ctx = ctx)

        val typesModule = codegenContext.symbolProvider.moduleForShape(codegenContext.model.lookup("test#TestStruct"))
        rustTemplate(
            """
            use aws_smithy_eventstream::frame::MarshallMessage;
            use aws_smithy_types::event_stream::{Message, Header, HeaderValue};
            use std::collections::HashMap;
            use aws_smithy_types::{Blob, DateTime};
            use ${typesModule.fullyQualifiedPath()}::*;

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

        unitTest("message_with_blob") {
            rustTemplate(
                """
                let event = TestStream::MessageWithBlob(
                    MessageWithBlob::builder().data(#{BlobInput:W}).build()
                );
                let result = $generator::new().marshall(event);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                let message = result.unwrap();
                let headers = headers_to_map(message.headers());
                assert_eq!(&str_header("event"), *headers.get(":message-type").unwrap());
                assert_eq!(&str_header("MessageWithBlob"), *headers.get(":event-type").unwrap());
                assert_eq!(&str_header("application/octet-stream"), *headers.get(":content-type").unwrap());
                assert_eq!(&b"hello, world!"[..], message.payload());
                """,
                "BlobInput" to builderInput("Blob::new(&b\"hello, world!\"[..])"),
            )
        }

        unitTest("message_with_string") {
            rustTemplate(
                """
                let event = TestStream::MessageWithString(
                    MessageWithString::builder().data(#{StringInput}).build()
                );
                let result = $generator::new().marshall(event);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                let message = result.unwrap();
                let headers = headers_to_map(message.headers());
                assert_eq!(&str_header("event"), *headers.get(":message-type").unwrap());
                assert_eq!(&str_header("MessageWithString"), *headers.get(":event-type").unwrap());
                assert_eq!(&str_header("text/plain"), *headers.get(":content-type").unwrap());
                assert_eq!(&b"hello, world!"[..], message.payload());
                """,
                "StringInput" to builderInput("\"hello, world!\""),
            )
        }

        unitTest("message_with_struct") {
            rustTemplate(
                """
                let event = TestStream::MessageWithStruct(
                    MessageWithStruct::builder().some_struct(#{StructInput}).build()
                );
                let result = $generator::new().marshall(event);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                let message = result.unwrap();
                let headers = headers_to_map(message.headers());
                assert_eq!(&str_header("event"), *headers.get(":message-type").unwrap());
                assert_eq!(&str_header("MessageWithStruct"), *headers.get(":event-type").unwrap());
                assert_eq!(&str_header(${testCase.eventStreamMessageContentType.dq()}), *headers.get(":content-type").unwrap());

                validate_body(
                    message.payload(),
                    ${testCase.validTestStruct.dq()},
                    MediaType::from(${testCase.mediaType.dq()})
                ).unwrap();
                """,
                "StructInput" to
                    builderInput(
                        """
                        TestStruct::builder()
                            .some_string(#{StringInput})
                            .some_int(#{IntInput})
                            .build()
                        """,
                        "IntInput" to builderInput("5"),
                        "StringInput" to builderInput("\"hello\""),
                    ),
            )
        }

        unitTest("message_with_union") {
            rustTemplate(
                """
                let event = TestStream::MessageWithUnion(MessageWithUnion::builder()
                    .some_union(#{UnionInput})
                    .build()
                );
                let result = $generator::new().marshall(event);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                let message = result.unwrap();
                let headers = headers_to_map(message.headers());
                assert_eq!(&str_header("event"), *headers.get(":message-type").unwrap());
                assert_eq!(&str_header("MessageWithUnion"), *headers.get(":event-type").unwrap());
                assert_eq!(&str_header(${testCase.eventStreamMessageContentType.dq()}), *headers.get(":content-type").unwrap());

                validate_body(
                    message.payload(),
                    ${testCase.validTestUnion.dq()},
                    MediaType::from(${testCase.mediaType.dq()})
                ).unwrap();
                """,
                "UnionInput" to builderInput("TestUnion::Foo(\"hello\".into())"),
            )
        }

        unitTest("message_with_headers") {
            rustTemplate(
                """
                let event = TestStream::MessageWithHeaders(MessageWithHeaders::builder()
                    .blob(#{BlobInput})
                    .boolean(#{BooleanInput})
                    .byte(#{ByteInput})
                    .int(#{IntInput})
                    .long(#{LongInput})
                    .short(#{ShortInput})
                    .string(#{StringInput})
                    .timestamp(#{TimestampInput})
                    .build()
                );
                let result = $generator::new().marshall(event);
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
                "BlobInput" to builderInput("Blob::new(&b\"test\"[..])"),
                "BooleanInput" to builderInput("true"),
                "ByteInput" to builderInput("55i8"),
                "IntInput" to builderInput("100_000i32"),
                "LongInput" to builderInput("9_000_000_000i64"),
                "ShortInput" to builderInput("16_000i16"),
                "StringInput" to builderInput("\"test\""),
                "TimestampInput" to builderInput("DateTime::from_secs(5)"),
            )
        }

        unitTest("message_with_header_and_payload") {
            rustTemplate(
                """
                let event = TestStream::MessageWithHeaderAndPayload(MessageWithHeaderAndPayload::builder()
                    .header(#{HeaderInput})
                    .payload(#{PayloadInput})
                    .build()
                );
                let result = $generator::new().marshall(event);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                let actual_message = result.unwrap();
                let expected_message = Message::new(&b"payload"[..])
                    .add_header(Header::new(":message-type", HeaderValue::String("event".into())))
                    .add_header(Header::new(":event-type", HeaderValue::String("MessageWithHeaderAndPayload".into())))
                    .add_header(Header::new("header", HeaderValue::String("header".into())))
                    .add_header(Header::new(":content-type", HeaderValue::String("application/octet-stream".into())));
                assert_eq!(expected_message, actual_message);
                """,
                "HeaderInput" to builderInput("\"header\""),
                "PayloadInput" to builderInput("Blob::new(&b\"payload\"[..])"),
            )
        }

        unitTest("message_with_no_header_payload_traits") {
            rustTemplate(
                """
                let event = TestStream::MessageWithNoHeaderPayloadTraits(MessageWithNoHeaderPayloadTraits::builder()
                    .some_int(#{IntInput})
                    .some_string(#{StringInput})
                    .build()
                );
                let result = $generator::new().marshall(event);
                assert!(result.is_ok(), "expected ok, got: {:?}", result);
                let message = result.unwrap();
                let headers = headers_to_map(message.headers());
                assert_eq!(&str_header("event"), *headers.get(":message-type").unwrap());
                assert_eq!(&str_header("MessageWithNoHeaderPayloadTraits"), *headers.get(":event-type").unwrap());
                assert_eq!(&str_header(${testCase.eventStreamMessageContentType.dq()}), *headers.get(":content-type").unwrap());

                validate_body(
                    message.payload(),
                    ${testCase.validMessageWithNoHeaderPayloadTraits.dq()},
                    MediaType::from(${testCase.mediaType.dq()})
                ).unwrap();
                """,
                "IntInput" to builderInput("5"),
                "StringInput" to builderInput("\"hello\""),
            )
        }
    }
}
