/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
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
        renderEventStreamBody = { writer, params ->
            writer.rustTemplate(
                """
                {
                    let error_marshaller = #{errorMarshallerConstructorFn}();
                    let marshaller = #{marshallerConstructorFn}();
                    let (signer, signer_sender) = #{DeferredSigner}::new();
                    _cfg.interceptor_state().store_put(signer_sender);
                    #{SdkBody}::from_body_1_x(#{http_body_util}::StreamBody::new(#{event_stream:W}))
                }
                """,
                "http_body_util" to CargoDependency.HttpBodyUtil01x.toType(),
                "SdkBody" to
                    CargoDependency.smithyTypes(codegenContext.runtimeConfig).withFeature("http-body-1-x")
                        .toType().resolve("body::SdkBody"),
                "aws_smithy_http" to RuntimeType.smithyHttp(codegenContext.runtimeConfig),
                "DeferredSigner" to
                    RuntimeType.smithyEventStream(codegenContext.runtimeConfig)
                        .resolve("frame::DeferredSigner"),
                "marshallerConstructorFn" to params.eventStreamMarshallerGenerator.render(),
                "errorMarshallerConstructorFn" to params.errorMarshallerConstructorFn,
                "event_stream" to (
                    eventStreamWithInitialRequest(codegenContext, protocol, params)
                        ?: messageStreamAdaptor(params.outerName, params.memberName)
                ),
            )
        },
    )

private fun eventStreamWithInitialRequest(
    codegenContext: ClientCodegenContext,
    protocol: Protocol,
    params: EventStreamBodyParams,
): Writable? {
    val parser = protocol.structuredDataSerializer().operationInputSerializer(params.operationShape) ?: return null

    if (codegenContext.protocolImpl?.httpBindingResolver?.handlesEventStreamInitialRequest(params.operationShape) != true) {
        return null
    }

    val smithyHttp = RuntimeType.smithyHttp(codegenContext.runtimeConfig)
    val eventOrInitial = smithyHttp.resolve("event_stream::EventOrInitial")
    val eventOrInitialMarshaller = smithyHttp.resolve("event_stream::EventOrInitialMarshaller")

    return writable {
        rustTemplate(
            """
            {
                use #{futures_util}::StreamExt;
                let body = #{parser}(&input)?;
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
            "parser" to parser,
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
