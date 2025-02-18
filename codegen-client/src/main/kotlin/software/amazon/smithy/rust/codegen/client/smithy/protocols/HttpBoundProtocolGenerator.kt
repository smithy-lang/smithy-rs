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
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.EventStreamBodyParams
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBoundProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.needsToHandleEventStreamInitialMessage

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
                    #{SdkBody}::from_body_0_4(#{hyper}::Body::wrap_stream(#{event_stream:W}))
                }
                """,
                "hyper" to CargoDependency.HyperWithStream.toType(),
                "SdkBody" to
                    CargoDependency.smithyTypes(codegenContext.runtimeConfig).withFeature("http-body-0-4-x")
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

    if (!params.operationShape.inputShape(codegenContext.model)
            .needsToHandleEventStreamInitialMessage(codegenContext.model, codegenContext.protocol)
    ) {
        return null
    }

    return writable {
        rustTemplate(
            """
            {
                use #{futures_util}::StreamExt;
                let body = #{parser}(&input)?;
                let initial_message = #{initial_message}(body);
                let mut buffer = Vec::new();
                #{write_message_to}(&initial_message, &mut buffer)?;
                let initial_message_stream = futures_util::stream::iter(vec![Ok(buffer.into())]);
                let adapter = #{message_stream_adaptor:W};
                initial_message_stream.chain(adapter)
            }
            """,
            "futures_util" to CargoDependency.FuturesUtil.toType(),
            "initial_message" to params.eventStreamMarshallerGenerator.renderInitialMessageGenerator(params.payloadContentType),
            "message_stream_adaptor" to messageStreamAdaptor(params.outerName, params.memberName),
            "parser" to parser,
            "write_message_to" to
                RuntimeType.smithyEventStream(codegenContext.runtimeConfig)
                    .resolve("frame::write_message_to"),
        )
    }
}

private fun messageStreamAdaptor(
    outerName: String,
    memberName: String,
) = writable {
    rust("$outerName.$memberName.into_body_stream(marshaller, error_marshaller, signer)")
}
