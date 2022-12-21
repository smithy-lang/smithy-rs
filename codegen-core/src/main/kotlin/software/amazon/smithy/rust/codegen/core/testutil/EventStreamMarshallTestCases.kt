/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq

internal object EventStreamMarshallTestCases {
    internal fun RustWriter.writeMarshallTestCases(
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
