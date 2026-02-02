/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolLoader
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.core.util.isOutputEventStream
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator

class StreamPayloadSerializerCustomization : ServerHttpBoundProtocolCustomization() {
    override fun section(section: ServerHttpBoundProtocolSection): Writable =
        when (section) {
            is ServerHttpBoundProtocolSection.WrapStreamPayload ->
                writable {
                    if (section.params.shape.isOutputEventStream(section.params.codegenContext.model)) {
                        // Event stream payload, of type `aws_smithy_http::event_stream::MessageStreamAdapter`, already
                        // implements the `Stream` trait, so no need to wrap it in the new-type.
                        section.params.payloadGenerator.generatePayload(
                            this,
                            section.params.shapeName,
                            section.params.shape,
                        )
                    } else {
                        // Otherwise, the stream payload is `aws_smithy_types::byte_stream::ByteStream`. We wrap it in the
                        // new-type to enable the `Stream` trait.
                        withBlockTemplate(
                            "#{FuturesStreamCompatByteStream}::new(",
                            ")",
                            "FuturesStreamCompatByteStream" to RuntimeType.futuresStreamCompatByteStream(section.params.codegenContext.runtimeConfig),
                        ) {
                            section.params.payloadGenerator.generatePayload(
                                this,
                                section.params.shapeName,
                                section.params.shape,
                            )
                        }
                    }
                }

            else -> emptySection
        }
}

class ServerProtocolLoader(supportedProtocols: ProtocolMap<ServerProtocolGenerator, ServerCodegenContext>) :
    ProtocolLoader<ServerProtocolGenerator, ServerCodegenContext>(supportedProtocols) {
    companion object {
        /**
         * Generate a protocol suffix for multi-protocol code generation.
         * For example: aws.protocols#restJson1 -> RestJson1
         *
         * This is used to differentiate between generated types (like event stream marshallers)
         * when a service supports multiple protocols.
         */
        private fun protocolSuffix(protocol: ShapeId): String {
            return protocol.name.toPascalCase()
        }

        /**
         * Returns the protocol suffix for multi-protocol support if the service has multiple protocols,
         * or null for single-protocol services.
         *
         * This is computed by checking how many supported protocols the service has.
         */
        fun protocolSuffixForCodegenContext(codegenContext: ServerCodegenContext): String? {
            val serviceProtocols = ServiceIndex.of(codegenContext.model).getProtocols(codegenContext.serviceShape)
            val supportedProtocolIds = setOf(
                RestJson1Trait.ID,
                RestXmlTrait.ID,
                AwsJson1_0Trait.ID,
                AwsJson1_1Trait.ID,
                Rpcv2CborTrait.ID,
            )
            val matchingProtocols = serviceProtocols.keys.filter { it in supportedProtocolIds }
            return if (matchingProtocols.size > 1) {
                protocolSuffix(codegenContext.protocol)
            } else {
                null
            }
        }

        fun defaultProtocols(
            httpBindingCustomizations: (ShapeId) -> List<HttpBindingCustomization> = { _ -> listOf() },
        ) = mapOf(
            RestJson1Trait.ID to
                    ServerRestJsonFactory(
                        additionalServerHttpBoundProtocolCustomizations =
                            listOf(
                                StreamPayloadSerializerCustomization(),
                            ),
                        additionalHttpBindingCustomizations = httpBindingCustomizations(RestJson1Trait.ID),
                    ),
            RestXmlTrait.ID to
                    ServerRestXmlFactory(
                        additionalServerHttpBoundProtocolCustomizations =
                            listOf(
                                StreamPayloadSerializerCustomization(),
                            ),
                    ),
            AwsJson1_0Trait.ID to
                    ServerAwsJsonFactory(
                        AwsJsonVersion.Json10,
                        additionalServerHttpBoundProtocolCustomizations = listOf(StreamPayloadSerializerCustomization()),
                        additionalHttpBindingCustomizations = httpBindingCustomizations(AwsJson1_0Trait.ID),
                    ),
            AwsJson1_1Trait.ID to
                    ServerAwsJsonFactory(
                        AwsJsonVersion.Json11,
                        additionalServerHttpBoundProtocolCustomizations = listOf(StreamPayloadSerializerCustomization()),
                        additionalHttpBindingCustomizations = httpBindingCustomizations(AwsJson1_1Trait.ID),
                    ),
            Rpcv2CborTrait.ID to
                    ServerRpcV2CborFactory(
                        additionalServerHttpBoundProtocolCustomizations =
                            listOf(
                                StreamPayloadSerializerCustomization(),
                            ),
                        additionalHttpBindingCustomizations = httpBindingCustomizations(Rpcv2CborTrait.ID),
                    ),
        )
    }
}
