/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.smithy.generators.HttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory

typealias ProtocolMap = Map<ShapeId, ProtocolGeneratorFactory<HttpProtocolGenerator>>

// typealias ProtocolMap =
// TODO: supportedProtocols must be runtime loadable via SPI; 2d
class ProtocolLoader(private val supportedProtocols: ProtocolMap) {
    fun protocolFor(
        model: Model,
        serviceShape: ServiceShape
    ): Pair<ShapeId, ProtocolGeneratorFactory<HttpProtocolGenerator>> {
        val protocols: MutableMap<ShapeId, Trait> = ServiceIndex.of(model).getProtocols(serviceShape)
        val matchingProtocols =
            protocols.keys.mapNotNull { protocolId -> supportedProtocols[protocolId]?.let { protocolId to it } }
        if (matchingProtocols.isEmpty()) {
            throw CodegenException("No matching protocol — service offers: ${protocols.keys}. We offer: ${supportedProtocols.keys}")
        }
        return matchingProtocols.first()
    }

    companion object {
        val DefaultProtocols = mapOf(
            AwsJson1_0Trait.ID to BasicAwsJsonFactory(AwsJsonVersion.Json10),
            AwsJson1_1Trait.ID to BasicAwsJsonFactory(AwsJsonVersion.Json11),
            RestJson1Trait.ID to AwsRestJsonFactory()
        )
        val Default = ProtocolLoader(DefaultProtocols)
    }
}
