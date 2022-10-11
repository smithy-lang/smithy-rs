/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolTestGenerator

/**
 * ServerServiceGenerator
 *
 * Service generator is the main code generation entry point for Smithy services. Individual structures and unions are
 * generated in codegen visitor, but this class handles all protocol-specific code generation (i.e. operations).
 */
open class ServerServiceGenerator(
    private val rustCrate: RustCrate,
    private val protocolGenerator: ServerProtocolGenerator,
    private val protocolSupport: ProtocolSupport,
    val protocol: ServerProtocol,
    private val codegenContext: CodegenContext,
) {
    private val index = TopDownIndex.of(codegenContext.model)
    protected val operations = index.getContainedOperations(codegenContext.serviceShape).sortedBy { it.id }

    /**
     * Render Service Specific code. Code will end up in different files via [useShapeWriter]. See `SymbolVisitor.kt`
     * which assigns a symbol location to each shape.
     */
    fun render() {
        rustCrate.withModule(RustModule.operation(Visibility.PRIVATE)) { writer ->
            ServerProtocolTestGenerator(codegenContext, protocolSupport, protocolGenerator).render(writer)
        }

        for (operation in operations) {
            if (operation.errors.isNotEmpty()) {
                rustCrate.withModule(RustModule.Error) { writer ->
                    renderCombinedErrors(writer, operation)
                }
            }
        }
        rustCrate.withModule(RustModule.private("operation_handler", "Operation handlers definition and implementation.")) { writer ->
            renderOperationHandler(writer, operations)
        }
        rustCrate.withModule(
            RustModule.public(
                "operation_registry",
                """
                Contains the [`operation_registry::OperationRegistry`], a place where
                you can register your service's operation implementations.
                """,
            ),
        ) { writer ->
            renderOperationRegistry(writer, operations)
        }

        // TODO(https://github.com/awslabs/smithy-rs/issues/1707): Remove, this is temporary.
        rustCrate.withModule(
            RustModule(
                "operation_shape",
                RustMetadata(
                    visibility = Visibility.PUBLIC,
                    additionalAttributes = listOf(
                        Attribute.DocHidden,
                    ),
                ),
                null,
            ),
        ) { writer ->
            for (operation in operations) {
                ServerOperationGenerator(codegenContext, operation).render(writer)
            }
        }

        // TODO(https://github.com/awslabs/smithy-rs/issues/1707): Remove, this is temporary.
        rustCrate.withModule(
            RustModule("service", RustMetadata(visibility = Visibility.PUBLIC, additionalAttributes = listOf(Attribute.DocHidden)), null),
        ) { writer ->
            ServerServiceGeneratorV2(
                codegenContext,
                protocol,
            ).render(writer)
        }

        renderExtras(operations)
    }

    // Render any extra section needed by subclasses of `ServerServiceGenerator`.
    open fun renderExtras(operations: List<OperationShape>) { }

    // Render combined errors.
    open fun renderCombinedErrors(writer: RustWriter, operation: OperationShape) {
        /* Subclasses can override */
    }

    // Render operations handler.
    open fun renderOperationHandler(writer: RustWriter, operations: List<OperationShape>) {
        ServerOperationHandlerGenerator(codegenContext, protocol, operations).render(writer)
    }

    // Render operations registry.
    private fun renderOperationRegistry(writer: RustWriter, operations: List<OperationShape>) {
        ServerOperationRegistryGenerator(codegenContext, protocol, operations).render(writer)
    }
}
