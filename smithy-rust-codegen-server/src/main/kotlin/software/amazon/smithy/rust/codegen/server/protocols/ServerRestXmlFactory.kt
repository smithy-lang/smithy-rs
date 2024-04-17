/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.protocols

import software.amazon.smithy.rust.codegen.core.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.server.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.generators.protocol.ServerRestXmlProtocol

/*
 * RestXml server-side protocol factory. This factory creates the [ServerHttpProtocolGenerator]
 * with RestXml specific configurations.
 */
class ServerRestXmlFactory(
    private val additionalServerHttpBoundProtocolCustomizations: List<ServerHttpBoundProtocolCustomization> = listOf(),
) : ProtocolGeneratorFactory<ServerHttpBoundProtocolGenerator, ServerCodegenContext> {
    override fun protocol(codegenContext: ServerCodegenContext): Protocol = ServerRestXmlProtocol(codegenContext)

    override fun buildProtocolGenerator(codegenContext: ServerCodegenContext): ServerHttpBoundProtocolGenerator =
        ServerHttpBoundProtocolGenerator(
            codegenContext,
            ServerRestXmlProtocol(codegenContext),
            additionalServerHttpBoundProtocolCustomizations,
        )

    override fun support(): ProtocolSupport {
        return ProtocolSupport(
            // Client support
            requestSerialization = false,
            requestBodySerialization = false,
            responseDeserialization = false,
            errorDeserialization = false,
            // Server support
            requestDeserialization = true,
            requestBodyDeserialization = true,
            responseSerialization = true,
            errorSerialization = true,
        )
    }
}
