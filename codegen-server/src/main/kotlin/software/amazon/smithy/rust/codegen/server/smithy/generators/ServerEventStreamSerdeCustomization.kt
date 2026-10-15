/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.EventStreamSerdeCustomization
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape

/** Server schema walkers and capability access for the shared frame generators. */
class ServerEventStreamSerdeCustomization(private val context: ServerCodegenContext) : EventStreamSerdeCustomization {
    override val contextType = ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType().resolve("schema::SharedServerProtocol")
    private val schema = RuntimeType.smithySchema(context.runtimeConfig)
    private val scope =
        arrayOf(
            "Error" to RuntimeType.smithyEventStream(context.runtimeConfig).resolve("error::Error"),
            "Bytes" to RuntimeType.Bytes,
            "ShapeSerializer" to schema.resolve("serde::ShapeSerializer"),
            "PayloadSerializer" to schema.resolve("codec::PayloadSerializer"),
        )

    override fun mediaType(unmarshalling: Boolean) =
        writable {
            val error = if (unmarshalling) "unmarshalling" else "marshalling"
            rustTemplate(
                """self.protocol.event_stream().ok_or_else(|| #{Error}::$error("protocol does not support event streams".to_owned()))?.event_stream_media_type()""",
                *scope,
            )
        }

    override fun serialize(
        target: Shape,
        value: String,
    ) = writable {
        rustTemplate(
            """
            {
                let capability = self.protocol.event_stream()
                    .ok_or_else(|| #{Error}::marshalling("protocol does not support event streams".to_owned()))?;
                let mut ser = capability.payload_codec().create_serializer();
                #{ShapeSerializer}::write_struct(&mut *ser, #{Target}::SCHEMA, &$value)
                    .map_err(|err| #{Error}::marshalling(format!("{err}")))?;
                #{Bytes}::from(#{PayloadSerializer}::finish_boxed(ser))
            }
            """,
            "Target" to context.symbolProvider.toSymbol(target),
            *scope,
        )
    }

    override fun deserialize(
        target: Shape,
        exception: Boolean,
    ) = writable {
        val kind = if (exception) "exception" else "event payload"
        rustTemplate(
            """
            {
                let capability = self.protocol.event_stream()
                    .ok_or_else(|| #{Error}::unmarshalling("protocol does not support event streams"))?;
                let mut deser = capability.payload_codec().create_deserializer(&message.payload()[..]);
                let parsed = #{Target}::deserialize_schema(&mut *deser)
                    .map_err(|err| #{Error}::unmarshalling(format!("failed to unmarshall $kind: {err}")))?;
                #{finish}
            }
            """,
            "Target" to context.symbolProvider.toSymbol(target),
            "finish" to
                writable {
                    if (target is StructureShape && target.canReachConstrainedShape(context.model, context.symbolProvider)) {
                        rustTemplate(
                            """parsed.build().map_err(|err| #{Error}::unmarshalling(format!("failed to unmarshall $kind due to constraint violation: {err}")))?""",
                            *scope,
                        )
                    } else {
                        rustTemplate("parsed")
                    }
                },
            *scope,
        )
    }
}
