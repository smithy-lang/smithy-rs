/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfigGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ProtocolTestGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ServiceErrorGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.util.inputShape

/**
 * ServiceGenerator
 *
 * Service generator is the main code generation entry point for Smithy services. Individual structures and unions are
 * generated in codegen visitor, but this class handles all protocol-specific code generation.
 */
class ServiceGenerator(
    private val rustCrate: RustCrate,
    private val protocolGenerator: ClientProtocolGenerator,
    private val protocolSupport: ProtocolSupport,
    private val clientCodegenContext: ClientCodegenContext,
    private val decorator: ClientCodegenDecorator,
) {
    private val index = TopDownIndex.of(clientCodegenContext.model)

    /**
     * Render Service-specific code. Code will end up in different files via `useShapeWriter`. See `SymbolVisitor.kt`
     * which assigns a symbol location to each shape.
     */
    fun render() {
        val operations = index.getContainedOperations(clientCodegenContext.serviceShape).sortedBy { it.id }
        operations.map { operation ->
            rustCrate.useShapeWriter(operation) operationWriter@{
                rustCrate.useShapeWriter(operation.inputShape(clientCodegenContext.model)) inputWriter@{
                    // Render the operation shape & serializers input `input.rs`
                    protocolGenerator.renderOperation(
                        this@operationWriter,
                        this@inputWriter,
                        operation,
                        decorator.operationCustomizations(clientCodegenContext, operation, listOf()),
                    )

                    // render protocol tests into `operation.rs` (note operationWriter vs. inputWriter)
                    ProtocolTestGenerator(clientCodegenContext, protocolSupport, operation, this@operationWriter).render()
                }
            }
        }

        ServiceErrorGenerator(clientCodegenContext, operations).render(rustCrate)

        rustCrate.withModule(RustModule.Config) {
            ServiceConfigGenerator.withBaseBehavior(
                clientCodegenContext,
                extraCustomizations = decorator.configCustomizations(clientCodegenContext, listOf()),
            ).render(this)
        }

        rustCrate.lib {
            Attribute.DocInline.render(this)
            write("pub use config::Config;")
        }
    }
}
