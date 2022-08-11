/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.smithy.protocols.restJsonFieldName
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator

/**
 * RestJson1 server-side protocol factory. This factory creates the [ServerHttpProtocolGenerator]
 * with RestJson1 specific configurations.
 */
class ServerRestJsonFactory : ProtocolGeneratorFactory<ServerHttpBoundProtocolGenerator, ServerCodegenContext> {
    override fun protocol(codegenContext: ServerCodegenContext): Protocol = ServerRestJson(codegenContext)

    override fun buildProtocolGenerator(codegenContext: ServerCodegenContext): ServerHttpBoundProtocolGenerator =
        ServerHttpBoundProtocolGenerator(codegenContext, ServerRestJson(codegenContext))

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

class ServerRestJson(private val serverCodegenContext: ServerCodegenContext) : RestJson(serverCodegenContext) {
    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator =
        JsonSerializerGenerator(
            serverCodegenContext,
            httpBindingResolver,
            ::restJsonFieldName,
            publicConstrainedTypes = serverCodegenContext.settings.codegenConfig.publicConstrainedTypes,
        )
}
