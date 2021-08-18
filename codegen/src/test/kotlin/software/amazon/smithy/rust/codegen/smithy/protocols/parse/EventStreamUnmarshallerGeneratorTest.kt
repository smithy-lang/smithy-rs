/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parse

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.protocols.EventStreamTestModels
import software.amazon.smithy.rust.codegen.smithy.protocols.EventStreamTestTools
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.unitTest

class EventStreamUnmarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(EventStreamTestModels.ModelArgumentsProvider::class)
    fun test(testCase: EventStreamTestModels.TestCase) {
        val test = EventStreamTestTools.generateTestProject(testCase.model)

        val protocolConfig = ProtocolConfig(
            test.model,
            test.symbolProvider,
            TestRuntimeConfig,
            test.serviceShape,
            ShapeId.from(testCase.protocolShapeId),
            "test"
        )
        val protocol = testCase.protocolBuilder(protocolConfig)
        val generator = EventStreamUnmarshallerGenerator(
            protocol,
            test.model,
            TestRuntimeConfig,
            test.symbolProvider,
            test.operationShape,
            test.streamShape
        )

        test.project.lib { writer ->
            // TODO(EventStream): Add test for MessageWithString
            // TODO(EventStream): Add test for MessageWithStruct
            // TODO(EventStream): Add test for MessageWithUnion
            // TODO(EventStream): Add test for MessageWithHeaders
            // TODO(EventStream): Add test for MessageWithHeaderAndPayload
            // TODO(EventStream): Add test for MessageWithNoHeaderPayloadTraits
            writer.unitTest(
                """
                use smithy_eventstream::frame::{Header, HeaderValue, Message, UnmarshallMessage, UnmarshalledMessage};
                use smithy_types::Blob;
                use crate::model::*;

                let message = Message::new(&b"hello, world!"[..])
                    .add_header(Header::new(":message-type", HeaderValue::String("event".into())))
                    .add_header(Header::new(":event-type", HeaderValue::String("MessageWithBlob".into())))
                    .add_header(Header::new(":content-type", HeaderValue::String("application/octet-stream".into())));
                let unmarshaller = ${writer.format(generator.render())}();
                let result = unmarshaller.unmarshall(&message).unwrap();
                if let UnmarshalledMessage::Event(event) = result {
                    assert_eq!(
                        TestStream::MessageWithBlob(
                            MessageWithBlob::builder().data(Blob::new(&b"hello, world!"[..])).build()
                        ),
                        event
                    );
                } else {
                    panic!("Expected event, got error: {:?}", result);
                }
                """,
                "message_with_blob",
            )
        }
        test.project.compileAndTest()
    }
}
