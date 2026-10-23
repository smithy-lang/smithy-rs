/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.EventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamErrorMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.util.lookup

class EventStreamSerdeModuleTest {
    private val model =
        EventStreamNormalizer.transform(
            """
            ${'$'}version: "2"

            namespace test

            use aws.protocols#restJson1

            @restJson1
            service TestService {
                operations: [TestOperation]
            }

            operation TestOperation {
                output := {
                    @httpPayload
                    events: Events
                }
            }

            structure Event {}

            @error("client")
            structure EventError {}

            @streaming
            union Events {
                event: Event
                error: EventError
            }
            """.asSmithyModel(),
        )

    @Test
    fun `event stream generators can use an arbitrary module`() {
        val protocolCodegenModules =
            ProtocolCodegenModules.under(RustModule.private("protocol_custom"))
        protocolCodegenModules.serde.fullyQualifiedPath() shouldBe
            "crate::protocol_custom::protocol_serde"
        val context =
            testCodegenContext(
                model,
                serviceShape = model.lookup("test#TestService"),
                protocolCodegenModules = protocolCodegenModules,
            )
        val protocol = RestJson(context)
        val operation = model.lookup<OperationShape>("test#TestOperation")
        val union = model.lookup<UnionShape>("test#Events")
        val serializer = protocol.structuredDataSerializer()

        val unmarshaller =
            EventStreamUnmarshallerGenerator(
                protocol,
                context,
                operation,
                union,
            ).render()
        val marshaller =
            EventStreamMarshallerGenerator(
                context,
                union,
                serializer,
                "application/json",
            ).render()
        val errorMarshaller =
            EventStreamErrorMarshallerGenerator(
                context,
                union,
                serializer,
                "application/json",
            ).render()

        unmarshaller.render() shouldBe
            "crate::protocol_custom::event_stream_serde::EventsUnmarshaller::new"
        marshaller.render() shouldBe
            "crate::protocol_custom::event_stream_serde::EventsMarshaller::new"
        errorMarshaller.render() shouldBe
            "crate::protocol_custom::event_stream_serde::EventsErrorMarshaller::new"
    }
}
