/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext

open class ProtocolLoader<T, C : CodegenContext>(private val supportedProtocols: ProtocolMap<T, C>) {
    fun protocolFor(
        model: Model,
        serviceShape: ServiceShape,
    ): Pair<ShapeId, ProtocolGeneratorFactory<T, C>> {
        val serviceProtocols: MutableMap<ShapeId, Trait> = ServiceIndex.of(model).getProtocols(serviceShape)
        val matchingProtocols =
            supportedProtocols.mapNotNull { (protocolId, factory) ->
                serviceProtocols[protocolId]?.let { protocolId to factory }
            }
        if (matchingProtocols.isEmpty()) {
            throw CodegenException("No matching protocol — service offers: ${serviceProtocols.keys}. We offer: ${supportedProtocols.keys}")
        }
        return matchingProtocols.first()
    }

    /**
     * Returns ALL matching protocols for a service, enabling multi-protocol code generation.
     * This is used when a service defines multiple protocols (e.g., both RestJson1 and RpcV2Cbor).
     */
    fun protocolsFor(
        model: Model,
        serviceShape: ServiceShape,
    ): List<Pair<ShapeId, ProtocolGeneratorFactory<T, C>>> {
        val serviceProtocols: MutableMap<ShapeId, Trait> = ServiceIndex.of(model).getProtocols(serviceShape)
        val matchingProtocols =
            supportedProtocols.mapNotNull { (protocolId, factory) ->
                serviceProtocols[protocolId]?.let { protocolId to factory }
            }
        if (matchingProtocols.isEmpty()) {
            throw CodegenException("No matching protocol — service offers: ${serviceProtocols.keys}. We offer: ${supportedProtocols.keys}")
        }
        return matchingProtocols
    }
}
