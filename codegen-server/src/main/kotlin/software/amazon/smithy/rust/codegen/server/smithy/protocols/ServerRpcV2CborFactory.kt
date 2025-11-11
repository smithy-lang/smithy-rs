/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerRpcV2CborProtocol

class ServerRpcV2CborFactory(
    private val additionalServerHttpBoundProtocolCustomizations: List<ServerHttpBoundProtocolCustomization> =
        emptyList(),
    private val additionalHttpBindingCustomizations: List<HttpBindingCustomization> = listOf(),
) : ProtocolGeneratorFactory<ServerHttpBoundProtocolGenerator, ServerCodegenContext> {
    override fun protocol(codegenContext: ServerCodegenContext): Protocol = ServerRpcV2CborProtocol(codegenContext)

    override fun buildProtocolGenerator(codegenContext: ServerCodegenContext): ServerHttpBoundProtocolGenerator =
        ServerHttpBoundProtocolGenerator(
            codegenContext,
            ServerRpcV2CborProtocol(codegenContext),
            additionalServerHttpBoundProtocolCustomizations,
            additionalHttpBindingCustomizations = additionalHttpBindingCustomizations,
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
