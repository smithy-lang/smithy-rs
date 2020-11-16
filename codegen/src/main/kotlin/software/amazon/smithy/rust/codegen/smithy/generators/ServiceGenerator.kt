/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.writer.CodegenWriterDelegator
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait

class ServiceGenerator(
    private val writers: CodegenWriterDelegator<RustWriter>,
    private val protocolGenerator: HttpProtocolGenerator,
    private val config: ProtocolConfig
) {
    private val index = TopDownIndex(config.model)

    fun render() {
        val operations = index.getContainedOperations(config.serviceShape)
        operations.map { operation ->
            writers.useShapeWriter(operation) { writer ->
                protocolGenerator.renderOperation(writer, operation)
                HttpProtocolTestGenerator(config, operation, writer).render()
            }
        }
        renderBodies()
    }

    fun renderBodies() {
        val operations = index.getContainedOperations(config.serviceShape)
        val bodies = operations.map { config.model.expectShape(it.input.get()) }.map {
            it.expectTrait(SyntheticInputTrait::class.java)
        }.mapNotNull {
            it.body
        }.map {
            config.model.expectShape(it, StructureShape::class.java)
        }
        bodies.map { body ->
            writers.useShapeWriter(body) { writer ->
                with(config) {
                    StructureGenerator(model, symbolProvider, writer, body, renderBuilder = false).render()
                }
            }
        }
    }
}
