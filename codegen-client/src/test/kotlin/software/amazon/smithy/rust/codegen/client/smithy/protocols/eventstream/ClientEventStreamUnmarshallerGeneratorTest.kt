/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.eventstream

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.EventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestTools
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestVariety
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.TestEventStreamProject
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

class RefactoredClientEventStreamUnmarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(TestCasesProvider::class)
    fun `test event stream unmarshaller generator`(testCase: EventStreamTestModels.TestCase) {
        val model = EventStreamNormalizer.transform(testCase.model)

        fun renderGenerator(
            codegenContext: ClientCodegenContext,
            operationShape: OperationShape,
            streamShape: UnionShape,
            protocol: Protocol,
        ): RuntimeType {
            fun builderSymbol(shape: StructureShape): Symbol =
                shape.builderSymbol(codegenContext.symbolProvider)
            return EventStreamUnmarshallerGenerator(
                protocol,
                codegenContext,
                operationShape,
                streamShape,
                ::builderSymbol,
            ).render()
        }

        clientIntegrationTest(model, IntegrationTestParams(service = "test#TestService", addModuleToEventStreamAllowList = true)) { codegenContext, rustCrate ->
            val protocol = testCase.protocolBuilder(codegenContext)
            val operationShape = codegenContext.model.expectShape(ShapeId.from("test#TestStreamOp")) as OperationShape
            val streamShape = codegenContext.model.expectShape(ShapeId.from("test#TestStream")) as UnionShape
            val generator = renderGenerator(codegenContext, operationShape, streamShape, protocol)

            rustCrate.integrationTest("unmarshall") {
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
                    "message_with_blob",
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
            }
        }
    }
}

class ClientEventStreamUnmarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(TestCasesProvider::class)
    fun test(testCase: EventStreamTestModels.TestCase) {
        EventStreamTestTools.runTestCase(
            testCase,
            object : ClientEventStreamBaseRequirements() {
                override fun renderGenerator(
                    codegenContext: ClientCodegenContext,
                    project: TestEventStreamProject,
                    protocol: Protocol,
                ): RuntimeType {
                    fun builderSymbol(shape: StructureShape): Symbol = shape.builderSymbol(codegenContext.symbolProvider)
                    return EventStreamUnmarshallerGenerator(
                        protocol,
                        codegenContext,
                        project.operationShape,
                        project.streamShape,
                        ::builderSymbol,
                    ).render()
                }
            },
            CodegenTarget.CLIENT,
            EventStreamTestVariety.Unmarshall,
        )
    }
}
