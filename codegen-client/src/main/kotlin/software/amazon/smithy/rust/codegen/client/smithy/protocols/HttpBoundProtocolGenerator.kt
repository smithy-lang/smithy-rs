/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.EventStreamBodyParams
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBoundProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol

class ClientHttpBoundProtocolPayloadGenerator(
    private val codegenContext: ClientCodegenContext,
    protocol: Protocol,
) : ProtocolPayloadGenerator by HttpBoundProtocolPayloadGenerator(
        codegenContext, protocol, HttpMessageType.REQUEST,
        eventStreamUseSchemaSerde = SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext),
        renderEventStreamBody = { writer, params ->
            renderEventStreamBodyInline(writer, codegenContext, protocol, params)
        },
    )

private fun eventStreamWithInitialRequest(
    codegenContext: ClientCodegenContext,
    protocol: Protocol,
    params: EventStreamBodyParams,
): Writable? {
    if (codegenContext.protocolImpl?.httpBindingResolver?.handlesEventStreamInitialRequest(params.operationShape) != true) {
        return null
    }

    val useSchemaSerde = SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)
    val parser = protocol.structuredDataSerializer().operationInputSerializer(params.operationShape)
    // Legacy path requires the structured serializer to be available.
    if (!useSchemaSerde && parser == null) return null

    val smithyHttp = RuntimeType.smithyHttp(codegenContext.runtimeConfig)
    val eventOrInitial = smithyHttp.resolve("event_stream::EventOrInitial")
    val eventOrInitialMarshaller = smithyHttp.resolve("event_stream::EventOrInitialMarshaller")

    val operationSymbol = codegenContext.symbolProvider.toSymbol(params.operationShape)
    val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)

    val bodyExpr: Writable =
        if (useSchemaSerde) {
            writable {
                rustTemplate(
                    """
                    {
                        let codec = protocol.payload_codec()
                            .ok_or("protocol has no payload codec")?;
                        let mut ser = codec.create_serializer();
                        #{ShapeSerializer}::write_struct(
                            &mut *ser,
                            #{Operation}::INPUT_SCHEMA,
                            &input,
                        )?;
                        #{SdkBody}::from(#{PayloadSerializer}::finish_boxed(ser))
                    }
                    """,
                    "Operation" to operationSymbol,
                    "ShapeSerializer" to smithySchema.resolve("serde::ShapeSerializer"),
                    "PayloadSerializer" to smithySchema.resolve("codec::PayloadSerializer"),
                    "SdkBody" to
                        CargoDependency.smithyTypes(codegenContext.runtimeConfig).withFeature("http-body-1-x")
                            .toType().resolve("body::SdkBody"),
                )
            }
        } else {
            writable {
                rustTemplate("#{parser}(&input)?", "parser" to parser!!)
            }
        }

    return writable {
        rustTemplate(
            """
            {
                use #{futures_util}::StreamExt;
                let body = #{body:W};
                let initial_message = #{initial_message}(body);

                // Wrap the marshaller to handle both initial and regular messages
                let wrapped_marshaller = #{EventOrInitialMarshaller}::new(marshaller);

                // Create stream with initial message
                let initial_stream = #{futures_util}::stream::once(async move {
                    #{Ok}(#{EventOrInitial}::InitialMessage(initial_message))
                });

                // Extract inner stream and map events
                let event_stream = ${params.outerName}.${params.memberName}.into_inner()
                    .map(|result| result.map(#{EventOrInitial}::Event));

                // Chain streams and convert to EventStreamSender
                let combined = initial_stream.chain(event_stream);
                #{EventStreamSender}::from(combined)
                    .into_body_stream(wrapped_marshaller, error_marshaller, signer)
            }
            """,
            *preludeScope,
            "futures_util" to CargoDependency.FuturesUtil.toType(),
            "initial_message" to params.eventStreamMarshallerGenerator.renderInitialRequestGenerator(params.payloadContentType),
            "body" to bodyExpr,
            "EventOrInitial" to eventOrInitial,
            "EventOrInitialMarshaller" to eventOrInitialMarshaller,
            "EventStreamSender" to smithyHttp.resolve("event_stream::EventStreamSender"),
        )
    }
}

private fun messageStreamAdaptor(
    outerName: String,
    memberName: String,
) = writable {
    rust("$outerName.$memberName.into_body_stream(marshaller, error_marshaller, signer)")
}

/**
 * Renders an event stream request body directly, without going through the legacy
 * [HttpBoundProtocolPayloadGenerator]/[ProtocolPayloadGenerator] plumbing. Used by
 * the schema-serde [RequestSerializerGenerator] path so the event stream branch
 * does not depend on the legacy `bodyGenerator` parameter.
 */
fun renderClientEventStreamBody(
    writer: software.amazon.smithy.rust.codegen.core.rustlang.RustWriter,
    codegenContext: ClientCodegenContext,
    protocol: Protocol,
    operationShape: software.amazon.smithy.model.shapes.OperationShape,
    memberShape: software.amazon.smithy.model.shapes.MemberShape,
    outerName: String,
) {
    val model = codegenContext.model
    val symbolProvider = codegenContext.symbolProvider
    val httpBindingResolver = protocol.httpBindingResolver
    val memberName = symbolProvider.toMemberName(memberShape)
    val unionShape =
        model.expectShape(
            memberShape.target,
            software.amazon.smithy.model.shapes.UnionShape::class.java,
        )
    val payloadContentType =
        httpBindingResolver.eventStreamMessageContentType(memberShape)
            ?: throw software.amazon.smithy.codegen.core.CodegenException("event streams must set a content type")
    val useSchemaSerde = SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)
    val serializerGenerator = protocol.structuredDataSerializer()
    val errorMarshallerConstructorFn =
        software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamErrorMarshallerGenerator(
            model,
            software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget.CLIENT,
            codegenContext.runtimeConfig,
            symbolProvider,
            unionShape,
            serializerGenerator,
            payloadContentType,
            useSchemaSerde = useSchemaSerde,
        ).render()
    val eventStreamMarshallerGenerator =
        software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamMarshallerGenerator(
            model,
            software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget.CLIENT,
            codegenContext.runtimeConfig,
            symbolProvider,
            unionShape,
            serializerGenerator,
            payloadContentType,
            useSchemaSerde = useSchemaSerde,
        )
    val params =
        EventStreamBodyParams(
            outerName,
            memberName,
            operationShape,
            eventStreamMarshallerGenerator,
            errorMarshallerConstructorFn,
            payloadContentType,
            object : software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.AdditionalPayloadContext {},
        )
    renderEventStreamBodyInline(writer, codegenContext, protocol, params)
}

/** Shared event-stream-body emission used by both the legacy lambda and the schema-serde helper. */
private fun renderEventStreamBodyInline(
    writer: software.amazon.smithy.rust.codegen.core.rustlang.RustWriter,
    codegenContext: ClientCodegenContext,
    protocol: Protocol,
    params: EventStreamBodyParams,
) {
    val useSchemaSerde = SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)
    val marshallerNew =
        if (useSchemaSerde) {
            """
            let protocol = _cfg.load::<#{SharedClientProtocol}>()
                .expect("a SharedClientProtocol is required")
                .clone();
            let error_marshaller = #{errorMarshallerConstructorFn}(protocol.clone());
            let marshaller = #{marshallerConstructorFn}(protocol.clone());
            """
        } else {
            """
            let error_marshaller = #{errorMarshallerConstructorFn}();
            let marshaller = #{marshallerConstructorFn}();
            """
        }
    writer.rustTemplate(
        """
        {
            $marshallerNew
            let (signer, signer_sender) = #{DeferredSigner}::new();
            _cfg.interceptor_state().store_put(signer_sender);
            #{SdkBody}::from_body_1_x(#{http_body_util}::StreamBody::new(#{event_stream:W}))
        }
        """,
        "http_body_util" to CargoDependency.HttpBodyUtil01x.toType(),
        "SdkBody" to
            CargoDependency.smithyTypes(codegenContext.runtimeConfig).withFeature("http-body-1-x")
                .toType().resolve("body::SdkBody"),
        "DeferredSigner" to
            RuntimeType.smithyEventStream(codegenContext.runtimeConfig)
                .resolve("frame::DeferredSigner"),
        "SharedClientProtocol" to
            RuntimeType.smithySchema(codegenContext.runtimeConfig)
                .resolve("protocol::SharedClientProtocol"),
        "marshallerConstructorFn" to params.eventStreamMarshallerGenerator.render(),
        "errorMarshallerConstructorFn" to params.errorMarshallerConstructorFn,
        "event_stream" to (
            eventStreamWithInitialRequest(codegenContext, protocol, params)
                ?: messageStreamAdaptor(params.outerName, params.memberName)
        ),
    )
}
