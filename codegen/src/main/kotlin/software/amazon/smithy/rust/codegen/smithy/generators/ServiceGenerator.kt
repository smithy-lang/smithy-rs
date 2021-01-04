/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.writer.CodegenWriterDelegator
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfigGenerator
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.util.inputShape

class ServiceGenerator(
    private val writers: CodegenWriterDelegator<RustWriter>,
    private val protocolGenerator: HttpProtocolGenerator,
    private val protocolSupport: ProtocolSupport,
    private val config: ProtocolConfig,
    private val extraCustomizations: List<ConfigCustomization>,
) {
    private val index = TopDownIndex.of(config.model)

    fun render() {
        val operations = index.getContainedOperations(config.serviceShape).sortedBy { it.id }
        operations.map { operation ->
            writers.useShapeWriter(operation) { operationWriter ->
                writers.useShapeWriter(operation.inputShape(config.model)) { inputWriter ->
                    protocolGenerator.renderOperation(operationWriter, inputWriter, operation)
                    HttpProtocolTestGenerator(config, protocolSupport, operation, operationWriter).render()
                }
            }
            val sym = operation.errorSymbol(config.symbolProvider)
            writers.useFileWriter("src/error.rs", sym.namespace) { writer ->
                CombinedErrorGenerator(config.model, config.symbolProvider, operation).render(writer)
            }
        }
        renderBodies(operations)

        writers.useFileWriter("src/config.rs", "crate::config") { writer ->
            ServiceConfigGenerator.withBaseBehavior(config, extraCustomizations = extraCustomizations).render(writer)
        }
        writers.useFileWriter("src/lib.rs", "crate::lib") {
            it.write("pub use config::Config;")
        }
    }

    private fun renderBodies(operations: List<OperationShape>) {
        val inputBodies = operations.map { config.model.expectShape(it.input.get()) }.map {
            it.expectTrait(SyntheticInputTrait::class.java)
        }.mapNotNull { // mapNotNull is flatMap but for null `map { it }.filter { it != null }`
            it.body
        }.map { // Lookup the Body structure by its id
            config.model.expectShape(it, StructureShape::class.java)
        }
        val outputBodies = operations.map { config.model.expectShape(it.output.get()) }.map {
            it.expectTrait(SyntheticOutputTrait::class.java)
        }.mapNotNull { // mapNotNull is flatMap but for null `map { it }.filter { it != null }`
            it.body
        }.map { // Lookup the Body structure by its id
            config.model.expectShape(it, StructureShape::class.java)
        }
        (inputBodies + outputBodies).map { body ->
            // The body symbol controls its location, usually in the serializer module
            writers.useShapeWriter(body) { writer ->
                with(config) {
                    // Generate a body via the structure generator
                    StructureGenerator(model, symbolProvider, writer, body).render()
                }
            }
        }
    }
}
