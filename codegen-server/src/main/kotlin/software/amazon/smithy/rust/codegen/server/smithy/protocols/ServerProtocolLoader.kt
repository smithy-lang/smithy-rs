/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolMap

/*
 * Protocol dispatcher, responsible for protocol selection.
 */
class ServerProtocolLoader(private val supportedProtocols: ProtocolMap) {
    fun protocolFor(
        model: Model,
        serviceShape: ServiceShape
    ): Pair<ShapeId, ProtocolGeneratorFactory<ProtocolGenerator>> {
        val protocols: MutableMap<ShapeId, Trait> = ServiceIndex.of(model).getProtocols(serviceShape)
        val matchingProtocols =
            protocols.keys.mapNotNull { protocolId -> supportedProtocols[protocolId]?.let { protocolId to it } }
        if (matchingProtocols.isEmpty()) {
            throw CodegenException("No matching protocol — service offers: ${protocols.keys}. We offer: ${supportedProtocols.keys}")
        }
        val pair = matchingProtocols.first()
        // TODO: is there a better way than an unsafe cast here?
        return Pair(pair.first, pair.second)
    }

    companion object {
        val DefaultProtocols = mapOf(
            // TODO: support other protocols.
            RestJson1Trait.ID to ServerRestJsonFactory(),
            RestXmlTrait.ID to ServerRestXmlFactory(),
        )
    }
}
