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
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
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

            // NOTE: `test-util` deliberately does not enable `aws-smithy-runtime/test-util`, even
            // though that feature no longer implies `legacy-test-util`. It still reaches
            // `aws-smithy-http-client/test-util` -> `aws-smithy-protocol-test`, which puts http
            // 0.2.x back in the normal dependency tree. The http test utilities generated code and
            // tests use are declared as dev-dependencies instead, where they cannot leak into a
            // consumer's build. Callers needing the pre-1.x helpers opt in to `legacy-test-util`.
            rustCrate.mergeFeature(TestUtilFeature.copy(deps = listOf()))
            if (codegenContext.settings.codegenConfig.includeLegacyClient) {
                rustCrate.mergeFeature(
                    Feature("legacy-test-util", default = false, listOf("aws-smithy-runtime/legacy-test-util")),
                )
            }

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
