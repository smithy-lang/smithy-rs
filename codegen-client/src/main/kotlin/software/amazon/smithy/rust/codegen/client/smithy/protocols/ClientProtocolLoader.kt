/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.AwsQueryCompatibleTrait
import software.amazon.smithy.aws.traits.protocols.AwsQueryTrait
import software.amazon.smithy.aws.traits.protocols.Ec2QueryTrait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationGenerator
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsQueryCompatible
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsQueryProtocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Ec2QueryProtocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolLoader
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestXml
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RpcV2Cbor
import software.amazon.smithy.rust.codegen.core.util.hasTrait

class ClientProtocolLoader(supportedProtocols: ProtocolMap<OperationGenerator, ClientCodegenContext>) :
    ProtocolLoader<OperationGenerator, ClientCodegenContext>(supportedProtocols) {
    companion object {
        val DefaultProtocols =
            mapOf(
                Rpcv2CborTrait.ID to ClientRpcV2CborFactory(),
                AwsJson1_0Trait.ID to ClientAwsJsonFactory(AwsJsonVersion.Json10),
                AwsJson1_1Trait.ID to ClientAwsJsonFactory(AwsJsonVersion.Json11),
                RestJson1Trait.ID to ClientRestJsonFactory(),
                RestXmlTrait.ID to ClientRestXmlFactory(),
                AwsQueryTrait.ID to ClientAwsQueryFactory(),
                Ec2QueryTrait.ID to ClientEc2QueryFactory(),
            )
        val Default = ClientProtocolLoader(DefaultProtocols)
    }
}

private val CLIENT_PROTOCOL_SUPPORT =
    ProtocolSupport(
        // Client protocol codegen enabled
        requestSerialization = true,
        requestBodySerialization = true,
        responseDeserialization = true,
        errorDeserialization = true,
        // Server protocol codegen disabled
        requestDeserialization = false,
        requestBodyDeserialization = false,
        responseSerialization = false,
        errorSerialization = false,
    )

private class ClientAwsJsonFactory(private val version: AwsJsonVersion) :
    ProtocolGeneratorFactory<OperationGenerator, ClientCodegenContext> {
    override fun protocol(codegenContext: ClientCodegenContext): Protocol =
        if (compatibleWithAwsQuery(codegenContext.serviceShape, version)) {
            AwsQueryCompatible(codegenContext, AwsJson(codegenContext, version))
        } else {
            AwsJson(codegenContext, version)
        }

    override fun buildProtocolGenerator(codegenContext: ClientCodegenContext): OperationGenerator =
        OperationGenerator(codegenContext, protocol(codegenContext))

    override fun support(): ProtocolSupport = CLIENT_PROTOCOL_SUPPORT

    private fun compatibleWithAwsQuery(
        serviceShape: ServiceShape,
        version: AwsJsonVersion,
    ) = serviceShape.hasTrait<AwsQueryCompatibleTrait>() && version == AwsJsonVersion.Json10
}

private class ClientAwsQueryFactory : ProtocolGeneratorFactory<OperationGenerator, ClientCodegenContext> {
    override fun protocol(codegenContext: ClientCodegenContext): Protocol = AwsQueryProtocol(codegenContext)

    override fun buildProtocolGenerator(codegenContext: ClientCodegenContext): OperationGenerator =
        OperationGenerator(codegenContext, protocol(codegenContext))

    override fun support(): ProtocolSupport = CLIENT_PROTOCOL_SUPPORT
}

private class ClientRestJsonFactory : ProtocolGeneratorFactory<OperationGenerator, ClientCodegenContext> {
    override fun protocol(codegenContext: ClientCodegenContext): Protocol = RestJson(codegenContext)

    override fun buildProtocolGenerator(codegenContext: ClientCodegenContext): OperationGenerator =
        OperationGenerator(codegenContext, RestJson(codegenContext))

    override fun support(): ProtocolSupport = CLIENT_PROTOCOL_SUPPORT
}

private class ClientEc2QueryFactory : ProtocolGeneratorFactory<OperationGenerator, ClientCodegenContext> {
    override fun protocol(codegenContext: ClientCodegenContext): Protocol = Ec2QueryProtocol(codegenContext)

    override fun buildProtocolGenerator(codegenContext: ClientCodegenContext): OperationGenerator =
        OperationGenerator(codegenContext, protocol(codegenContext))

    override fun support(): ProtocolSupport = CLIENT_PROTOCOL_SUPPORT
}

class ClientRestXmlFactory(
    private val generator: (CodegenContext) -> Protocol = { RestXml(it) },
) : ProtocolGeneratorFactory<OperationGenerator, ClientCodegenContext> {
    override fun protocol(codegenContext: ClientCodegenContext): Protocol = generator(codegenContext)

    override fun buildProtocolGenerator(codegenContext: ClientCodegenContext): OperationGenerator =
        OperationGenerator(codegenContext, protocol(codegenContext))

    override fun support(): ProtocolSupport = CLIENT_PROTOCOL_SUPPORT
}

class ClientRpcV2CborFactory : ProtocolGeneratorFactory<OperationGenerator, ClientCodegenContext> {
    override fun protocol(codegenContext: ClientCodegenContext): Protocol = RpcV2Cbor(codegenContext)

    override fun buildProtocolGenerator(codegenContext: ClientCodegenContext): OperationGenerator =
        OperationGenerator(codegenContext, protocol(codegenContext))

    override fun support(): ProtocolSupport = CLIENT_PROTOCOL_SUPPORT
}
