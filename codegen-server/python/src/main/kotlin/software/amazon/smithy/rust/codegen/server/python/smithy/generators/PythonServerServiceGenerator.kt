/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerServiceGenerator
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver

/**
 * PythonServerServiceGenerator
 *
 * Service generator is the main codegeneration entry point for Smithy services. Individual structures and unions are
 * generated in codegen visitor, but this class handles all protocol-specific code generation (i.e. operations).
 */
class PythonServerServiceGenerator(
    private val rustCrate: RustCrate,
    private val protocolGenerator: ProtocolGenerator,
    private val protocolSupport: ProtocolSupport,
    private val httpBindingResolver: HttpBindingResolver,
    private val context: CodegenContext,
) : ServerServiceGenerator(rustCrate, protocolGenerator, protocolSupport, httpBindingResolver, context) {

    /**
     * Render Service Specific code. Code will end up in different files via [useShapeWriter]. See `SymbolVisitor.kt`
     * which assigns a symbol location to each shape.
     */
    override fun render() {
        super.render()
        rustCrate.withModule(RustModule.public("python_server_application", "Python server and application implementation.")) { writer ->
            PythonApplicationGenerator(context, operations)
                .render(writer)
        }
    }

    override fun renderCombineErrors(writer: RustWriter, operation: OperationShape) {
        PythonServerCombinedErrorGenerator(context.model, context, operation).render(writer)
    }

    override fun renderOperationHandler(writer: RustWriter, operations: List<OperationShape>) {
        PythonServerOperationHandlerGenerator(context, operations).render(writer)
    }
}
