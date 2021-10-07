/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfigGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.error.CombinedErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.error.TopLevelErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.util.inputShape

class ServiceGenerator(
    private val rustCrate: RustCrate,
    private val protocolGenerator: ProtocolGenerator,
    private val protocolSupport: ProtocolSupport,
    private val context: CodegenContext,
    private val decorator: RustCodegenDecorator,
) {
    private val index = TopDownIndex.of(context.model)

    fun render() {
        val operations = index.getContainedOperations(context.serviceShape).sortedBy { it.id }
        operations.map { operation ->
            rustCrate.useShapeWriter(operation) { operationWriter ->
                rustCrate.useShapeWriter(operation.inputShape(context.model)) { inputWriter ->
                    protocolGenerator.renderOperation(
                        operationWriter,
                        inputWriter,
                        operation,
                        decorator.operationCustomizations(context, operation, listOf())
                    )
                }
            }
            rustCrate.withModule(RustModule.Error) { writer ->
                CombinedErrorGenerator(context.model, context.symbolProvider, operation)
                    .render(writer)
            }
        }

        TopLevelErrorGenerator(context, operations).render(rustCrate)

        rustCrate.withModule(RustModule.Config) { writer ->
            ServiceConfigGenerator.withBaseBehavior(
                context,
                extraCustomizations = decorator.configCustomizations(context, listOf())
            )
                .render(writer)
        }

        rustCrate.lib { it.write("pub use config::Config;") }
    }
}
