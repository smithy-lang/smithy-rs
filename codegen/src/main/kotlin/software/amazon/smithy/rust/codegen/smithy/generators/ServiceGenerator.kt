/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfigGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.error.CombinedErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.error.TopLevelErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolTestGenerator
import software.amazon.smithy.rust.codegen.util.inputShape

/**
 * ServiceGenerator
 *
 * Service generator is the main code generation entry point for Smithy services. Individual structures and unions are
 * generated in codegen visitor, but this class handles all protocol-specific code generation.
 */
class ServiceGenerator(
    private val rustCrate: RustCrate,
    private val protocolGenerator: ProtocolGenerator,
    private val protocolSupport: ProtocolSupport,
    private val clientCodegenContext: ClientCodegenContext,
    private val decorator: RustCodegenDecorator<ClientCodegenContext>,
) {
    private val index = TopDownIndex.of(clientCodegenContext.model)

    /**
     * Render Service-specific code. Code will end up in different files via `useShapeWriter`. See `SymbolVisitor.kt`
     * which assigns a symbol location to each shape.
     */
    fun render() {
        val operations = index.getContainedOperations(clientCodegenContext.serviceShape).sortedBy { it.id }
        operations.map { operation ->
            rustCrate.useShapeWriter(operation) { operationWriter ->
                rustCrate.useShapeWriter(operation.inputShape(clientCodegenContext.model)) { inputWriter ->
                    // Render the operation shape & serializers input `input.rs`
                    protocolGenerator.renderOperation(
                        operationWriter,
                        inputWriter,
                        operation,
                        decorator.operationCustomizations(clientCodegenContext, operation, listOf())
                    )

                    // render protocol tests into `operation.rs` (note operationWriter vs. inputWriter)
                    ProtocolTestGenerator(clientCodegenContext, protocolSupport, operation, operationWriter).render()
                }
            }
            // Render a service-level error enum containing every error that the service can emit
            rustCrate.withModule(RustModule.Error) { writer ->
                CombinedErrorGenerator(clientCodegenContext.model, clientCodegenContext.symbolProvider, operation).render(writer)
            }
        }

        TopLevelErrorGenerator(clientCodegenContext, operations).render(rustCrate)

        rustCrate.withModule(RustModule.Config) { writer ->
            ServiceConfigGenerator.withBaseBehavior(
                clientCodegenContext,
                extraCustomizations = decorator.configCustomizations(clientCodegenContext, listOf())
            ).render(writer)
        }

        rustCrate.lib {
            Attribute.DocInline.render(it)
            it.write("pub use config::Config;")
        }
    }
}
