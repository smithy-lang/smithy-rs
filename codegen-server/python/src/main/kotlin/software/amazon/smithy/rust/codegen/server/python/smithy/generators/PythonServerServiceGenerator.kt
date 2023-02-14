/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerServiceGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator

/**
 * PythonServerServiceGenerator
 *
 * Service generator is the main code generation entry point for Smithy services. Individual structures and unions are
 * generated in codegen visitor, but this class handles all protocol-specific code generation (i.e. operations).
 */
class PythonServerServiceGenerator(
    private val rustCrate: RustCrate,
    protocolGenerator: ServerProtocolGenerator,
    protocolSupport: ProtocolSupport,
    protocol: ServerProtocol,
    private val context: ServerCodegenContext,
) : ServerServiceGenerator(rustCrate, protocolGenerator, protocolSupport, protocol, context) {

    override fun renderCombinedErrors(writer: RustWriter, operation: OperationShape) {
        PythonServerOperationErrorGenerator(context.model, context.symbolProvider, operation).render(writer)
    }

    override fun renderExtras(operations: List<OperationShape>) {
        rustCrate.withModule(RustModule.public("python_server_application", "Python server and application implementation.")) {
            PythonApplicationGenerator(context, protocol, operations)
                .render(this)
        }
    }
}
