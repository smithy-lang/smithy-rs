/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBoundProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.StreamPayloadSerializer

class ClientHttpBoundProtocolPayloadGenerator(
    codegenContext: ClientCodegenContext,
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
                let adapter: #{aws_smithy_http}::event_stream::MessageStreamAdapter<_, _> =
                    ${params.outerName}.${params.memberName}.into_body_stream(marshaller, error_marshaller, signer);
                let body: #{SdkBody} = #{hyper}::Body::wrap_stream(#{HyperBodyWrapEventStream}::new(adapter)).into();
                body
            }
            """,
            "hyper" to CargoDependency.HyperWithStream.toType(),
            "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
            "aws_smithy_http" to RuntimeType.smithyHttp(codegenContext.runtimeConfig),
            "DeferredSigner" to RuntimeType.smithyEventStream(codegenContext.runtimeConfig)
                .resolve("frame::DeferredSigner"),
            "HyperBodyWrapEventStream" to RuntimeType.hyperBodyWrapStream(codegenContext.runtimeConfig)
                .resolve("HyperBodyWrapEventStream"),
            "marshallerConstructorFn" to params.marshallerConstructorFn,
            "errorMarshallerConstructorFn" to params.errorMarshallerConstructorFn,
        )
    },
    streamPayloadSerializer = StreamPayloadSerializer(
        { writer, params ->
            // `aws_smithy_http::byte_stream::ByteStream` no longer implements `futures::stream::Stream`
            // so wrap it in a new-type to enable the trait.
            writer.rust(
                "#T",
                RuntimeType.hyperBodyWrapStream(params.runtimeConfig).resolve("HyperBodyWrapByteStream").toSymbol(),
            )
        },
        { writer, params ->
            writer.rust(
                "#T::new(${params.payloadName!!})",
                RuntimeType.hyperBodyWrapStream(params.runtimeConfig).resolve("HyperBodyWrapByteStream").toSymbol(),
            )
        },
    ),
)
