/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerRootGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator

/**
 * PythonServerServiceGenerator
 *
 * Service generator is the main code generation entry point for Smithy services. Individual structures and unions are
 * generated in codegen visitor, but this class handles all protocol-specific code generation (i.e. operations).
 */
class PythonServerRootGenerator(
    private val rustCrate: RustCrate,
    protocolGenerator: ServerProtocolGenerator,
    protocolSupport: ProtocolSupport,
    protocol: ServerProtocol,
    private val context: ServerCodegenContext,
) : ServerRootGenerator(rustCrate, protocolGenerator, protocolSupport, protocol, context) {
    override fun renderExtras(operations: List<OperationShape>) {
        rustCrate.withModule(PythonServerRustModule.PythonServerApplication) {
            PythonApplicationGenerator(context, protocol, operations)
                .render(this)
        }
    }
}
