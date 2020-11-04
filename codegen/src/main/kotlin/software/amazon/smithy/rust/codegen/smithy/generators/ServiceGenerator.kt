/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.codegen.core.writer.CodegenWriterDelegator
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.protocols.AwsJson10Factory
import software.amazon.smithy.rust.codegen.smithy.protocols.AwsRestJsonFactory

class ServiceGenerator(
    private val model: Model,
    private val symbolProvider: SymbolProvider,
    private val runtimeConfig: RuntimeConfig,
    private val serviceShape: ServiceShape,
    private val writers: CodegenWriterDelegator<RustWriter>
) {
    // TODO: refactor to be runtime pluggable; 2d
    private val index = TopDownIndex(model)
    private val supportedProtocols = mapOf(
        AwsJson1_0Trait.ID to AwsJson10Factory(),
        RestJson1Trait.ID to AwsRestJsonFactory()

    )
    private val protocols: MutableMap<ShapeId, Trait> = ServiceIndex(model).getProtocols(serviceShape)
    private val matchingProtocols = protocols.keys.mapNotNull { protocolId -> supportedProtocols[protocolId]?.let { protocolId to it } }

    init {
        if (matchingProtocols.isEmpty()) {
            throw CodegenException("No matching protocol — service offers: ${protocols.keys}. We offer: ${supportedProtocols.keys}")
        }
    }

    fun render() {
        val operations = index.getContainedOperations(serviceShape)
        val (protocol, generator) = matchingProtocols.first()
        // TODO: refactor so that we don't need to re-instantiate the protocol for every operation
        operations.map { operation ->
            writers.useShapeWriter(operation) { writer ->
                // transform ensures that all models have input shapes
                val input = operation.input.get().let { model.expectShape(it, StructureShape::class.java) }
                val config = ProtocolConfig(model, symbolProvider, runtimeConfig, writer, serviceShape, operation, input, protocol)
                generator.build(config).render()
                HttpProtocolTestGenerator(config).render()
            }
        }
    }
}
