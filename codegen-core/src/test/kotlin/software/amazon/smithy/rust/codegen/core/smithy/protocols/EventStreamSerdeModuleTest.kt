/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.EventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamErrorMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext

class EventStreamSerdeModuleTest {
    @Test
    fun `generates event stream types in the context module`() {
        val model =
            EventStreamNormalizer.transform(
                """
                ${'$'}version: "2"
                namespace test

                use aws.protocols#restJson1

                structure Event { value: String }

                @error("client")
                structure SomeError { message: String }

                @streaming
                union TestStream { event: Event }

                structure TestStreamInputOutput {
                    @required
                    @httpPayload
                    value: TestStream
                }

                @http(method: "POST", uri: "/test")
                operation TestStreamOp {
                    input: TestStreamInputOutput,
                    output: TestStreamInputOutput,
                    errors: [SomeError]
                }

                @restJson1
                service TestService {
                    version: "1",
                    operations: [TestStreamOp]
                }
                """.asSmithyModel(),
            )
        val eventStreamSerdeModule =
            RustModule.private("event_stream_serde", parent = RustModule.private("protocol_test"))
        val codegenContext =
            testCodegenContext(
                model,
                serviceShape =
                    model.expectShape(ShapeId.from("test#TestService"), ServiceShape::class.java),
                eventStreamSerdeModule = eventStreamSerdeModule,
            )
        val protocol = RestJson(codegenContext)
        val operationShape = model.expectShape(ShapeId.from("test#TestStreamOp"), OperationShape::class.java)
        val unionShape = model.expectShape(ShapeId.from("test#TestStream"), UnionShape::class.java)
        val serializerGenerator = protocol.structuredDataSerializer()
        val payloadContentType = "application/json"

        val marshaller =
            EventStreamMarshallerGenerator(
                model,
                codegenContext.target,
                codegenContext.runtimeConfig,
                codegenContext.symbolProvider,
                unionShape,
                serializerGenerator,
                payloadContentType,
                eventStreamSerdeModule = codegenContext.eventStreamSerdeModule,
            ).render()
        val errorMarshaller =
            EventStreamErrorMarshallerGenerator(
                model,
                codegenContext.target,
                codegenContext.runtimeConfig,
                codegenContext.symbolProvider,
                unionShape,
                serializerGenerator,
                payloadContentType,
                eventStreamSerdeModule = codegenContext.eventStreamSerdeModule,
            ).render()
        val unmarshaller =
            EventStreamUnmarshallerGenerator(
                protocol,
                codegenContext,
                operationShape,
                unionShape,
            ).render()

        marshaller.path shouldBe "crate::protocol_test::event_stream_serde::TestStreamMarshaller::new"
        errorMarshaller.path shouldBe "crate::protocol_test::event_stream_serde::TestStreamErrorMarshaller::new"
        unmarshaller.path shouldBe "crate::protocol_test::event_stream_serde::TestStreamUnmarshaller::new"
        (marshaller.dependency as InlineDependency).module shouldBe eventStreamSerdeModule
        (errorMarshaller.dependency as InlineDependency).module shouldBe eventStreamSerdeModule
        (unmarshaller.dependency as InlineDependency).module shouldBe eventStreamSerdeModule
    }
}
