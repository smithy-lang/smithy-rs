/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.protocols.RestXml

/*
 * RestXml server-side protocol factory. This factory creates the [ServerHttpProtocolGenerator]
 * with RestXml specific configurations.
 */
class ServerRestXmlFactory : ProtocolGeneratorFactory<ServerHttpBoundProtocolGenerator> {
    override fun protocol(codegenContext: CodegenContext): Protocol = RestXml(codegenContext)

    override fun buildProtocolGenerator(codegenContext: CodegenContext): ServerHttpBoundProtocolGenerator =
        ServerHttpBoundProtocolGenerator(codegenContext, RestXml(codegenContext))

    override fun transformModel(model: Model): Model = model

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
            errorSerialization = true
        )
    }
}
