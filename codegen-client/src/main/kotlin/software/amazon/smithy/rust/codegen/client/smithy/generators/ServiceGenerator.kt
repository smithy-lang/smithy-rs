/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.TestUtilFeature
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfigGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.error.ServiceErrorGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

/**
 * ServiceGenerator
 *
 * Service generator is the main code generation entry point for Smithy services. Individual structures and unions are
 * generated in codegen visitor, but this class handles all protocol-specific code generation.
 */
class ServiceGenerator(
    private val rustCrate: RustCrate,
    private val codegenContext: ClientCodegenContext,
    private val decorator: ClientCodegenDecorator,
) {
    private val index = TopDownIndex.of(codegenContext.model)

    /**
     * Render Service-specific code. Code will end up in different files via `useShapeWriter`. See `SymbolVisitor.kt`
     * which assigns a symbol location to each shape.
     */
    fun render() {
        val operations = index.getContainedOperations(codegenContext.serviceShape).sortedBy { it.id }
        ServiceErrorGenerator(
            codegenContext,
            operations,
            decorator.errorCustomizations(codegenContext, emptyList()),
        ).render(rustCrate)

        rustCrate.withModule(ClientRustModule.config) {
            val serviceConfigGenerator =
                ServiceConfigGenerator.withBaseBehavior(
                    codegenContext,
                    extraCustomizations = decorator.configCustomizations(codegenContext, listOf()),
                )
            serviceConfigGenerator.render(this)

            // Enable users to opt in to the `test-util` feature in the runtime crate
            rustCrate.mergeFeature(TestUtilFeature.copy(deps = listOf("aws-smithy-runtime/test-util")))

            ServiceRuntimePluginGenerator(codegenContext)
                .render(this, decorator.serviceRuntimePluginCustomizations(codegenContext, emptyList()))

            ConfigOverrideRuntimePluginGenerator(codegenContext)
                .render(this, decorator.configCustomizations(codegenContext, listOf()))
        }

        rustCrate.lib {
            Attribute.DocInline.render(this)
            write("pub use config::Config;")
        }

        ClientRuntimeTypesReExportGenerator(codegenContext, rustCrate).render()
    }
}
