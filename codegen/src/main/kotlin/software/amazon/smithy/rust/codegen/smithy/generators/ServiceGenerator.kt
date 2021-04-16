/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfigGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.error.CombinedErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.error.TopLevelErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.util.inputShape

class ServiceGenerator(
    private val rustCrate: RustCrate,
    private val protocolGenerator: HttpProtocolGenerator,
    private val protocolSupport: ProtocolSupport,
    private val config: ProtocolConfig,
    private val decorator: RustCodegenDecorator,
) {
    private val index = TopDownIndex.of(config.model)

    fun render() {
        val operations = index.getContainedOperations(config.serviceShape).sortedBy { it.id }
        operations.map { operation ->
            rustCrate.useShapeWriter(operation) { operationWriter ->
                rustCrate.useShapeWriter(operation.inputShape(config.model)) { inputWriter ->
                    protocolGenerator.renderOperation(
                        operationWriter,
                        inputWriter,
                        operation,
                        decorator.operationCustomizations(config, operation, listOf())
                    )
                    HttpProtocolTestGenerator(config, protocolSupport, operation, operationWriter).render()
                }
            }
            rustCrate.withModule(RustModule.Error) { writer ->
                CombinedErrorGenerator(config.model, config.symbolProvider, operation).render(writer)
            }
        }

        TopLevelErrorGenerator(config, operations).render(rustCrate)
        renderBodies(operations)

        rustCrate.withModule(RustModule.Config) { writer ->
            ServiceConfigGenerator.withBaseBehavior(
                config,
                extraCustomizations = decorator.configCustomizations(config, listOf())
            ).render(writer)
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
            rustCrate.useShapeWriter(body) { writer ->
                with(config) {
                    // Generate a body via the structure generator
                    StructureGenerator(model, symbolProvider, writer, body).render()
                }
            }
        }
    }
}
