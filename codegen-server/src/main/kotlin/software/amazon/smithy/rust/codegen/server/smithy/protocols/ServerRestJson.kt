/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.JsonParserCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.restJsonFieldName
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customizations.BeforeIteratingOverMapOrCollectionJsonCustomization
import software.amazon.smithy.rust.codegen.server.smithy.customizations.BeforeSerializingMemberJsonCustomization
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerRestJsonProtocol

/**
 * RestJson1 server-side protocol factory. This factory creates the [ServerHttpProtocolGenerator]
 * with RestJson1 specific configurations.
 */
class ServerRestJsonFactory(
    private val additionalParserCustomizations: List<JsonParserCustomization> = listOf(),
    private val additionalServerHttpBoundProtocolCustomizations: List<ServerHttpBoundProtocolCustomization> = listOf(),
    private val additionalHttpBindingCustomizations: List<HttpBindingCustomization> = listOf(),
) : ProtocolGeneratorFactory<ServerHttpBoundProtocolGenerator, ServerCodegenContext> {
    override fun protocol(codegenContext: ServerCodegenContext): Protocol = ServerRestJsonProtocol(codegenContext, additionalParserCustomizations)

    override fun buildProtocolGenerator(codegenContext: ServerCodegenContext): ServerHttpBoundProtocolGenerator =
        ServerHttpBoundProtocolGenerator(
            codegenContext,
            ServerRestJsonProtocol(
                codegenContext,
                additionalParserCustomizations,
            ),
            additionalServerHttpBoundProtocolCustomizations,
            additionalHttpBindingCustomizations,
        )

    override fun support(): ProtocolSupport {
        return ProtocolSupport(
            /* Client support */
            requestSerialization = false,
            requestBodySerialization = false,
            responseDeserialization = false,
            errorDeserialization = false,
            /* Server support */
            requestDeserialization = true,
            requestBodyDeserialization = true,
            responseSerialization = true,
            errorSerialization = true,
        )
    }
}

class ServerRestJsonSerializerGenerator(
    private val codegenContext: ServerCodegenContext,
    private val httpBindingResolver: HttpBindingResolver,
    private val jsonSerializerGenerator: JsonSerializerGenerator =
        JsonSerializerGenerator(
            codegenContext,
            httpBindingResolver,
            ::restJsonFieldName,
            customizations = listOf(
                BeforeIteratingOverMapOrCollectionJsonCustomization(codegenContext),
                BeforeSerializingMemberJsonCustomization(codegenContext),
            ),
        ),
) : StructuredDataSerializerGenerator by jsonSerializerGenerator
