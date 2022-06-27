/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolTestGenerator
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver

/**
 * ServerServiceGenerator
 *
 * Service generator is the main codegeneration entry point for Smithy services. Individual structures and unions are
 * generated in codegen visitor, but this class handles all protocol-specific code generation (i.e. operations).
 */
open class ServerServiceGenerator(
    private val rustCrate: RustCrate,
    private val protocolGenerator: ProtocolGenerator,
    private val protocolSupport: ProtocolSupport,
    private val httpBindingResolver: HttpBindingResolver,
    private val coreCodegenContext: CoreCodegenContext,
) {
    private val index = TopDownIndex.of(coreCodegenContext.model)
    protected val operations = index.getContainedOperations(coreCodegenContext.serviceShape).sortedBy { it.id }

    /**
     * Render Service Specific code. Code will end up in different files via [useShapeWriter]. See `SymbolVisitor.kt`
     * which assigns a symbol location to each shape.
     */
    fun render() {
        for (operation in operations) {
            rustCrate.useShapeWriter(operation) { operationWriter ->
                protocolGenerator.serverRenderOperation(
                    operationWriter,
                    operation,
                )
                ServerProtocolTestGenerator(coreCodegenContext, protocolSupport, operation, operationWriter)
                    .render()
            }
            if (operation.errors.isNotEmpty()) {
                rustCrate.withModule(RustModule.Error) { writer ->
                    renderCombinedErrors(writer, operation)
                }
            }
        }
        rustCrate.withModule(RustModule.public("operation_handler", "Operation handlers definition and implementation.")) { writer ->
            renderOperationHandler(writer, operations)
        }
        rustCrate.withModule(RustModule.public("operation_registry", "A registry of your service's operations.")) { writer ->
            renderOperationRegistry(writer, operations)
        }
        renderExtras(operations)
    }

    // Render any extra section needed by subclasses of `ServerServiceGenerator`.
    open fun renderExtras(operations: List<OperationShape>) { }

    // Render combined errors.
    open fun renderCombinedErrors(writer: RustWriter, operation: OperationShape) {
        ServerCombinedErrorGenerator(coreCodegenContext.model, coreCodegenContext.symbolProvider, operation).render(writer)
    }

    // Render operations handler.
    open fun renderOperationHandler(writer: RustWriter, operations: List<OperationShape>) {
        ServerOperationHandlerGenerator(coreCodegenContext, operations).render(writer)
    }

    // Render operations registry.
    private fun renderOperationRegistry(writer: RustWriter, operations: List<OperationShape>) {
        ServerOperationRegistryGenerator(coreCodegenContext, httpBindingResolver, operations).render(writer)
    }
}
