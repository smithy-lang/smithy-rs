/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.util.serviceNameOrDefault

class FluentClientDecorator : ClientCodegenDecorator {
    override val name: String = "FluentClient"
    override val order: Byte = 0

    private fun applies(codegenContext: ClientCodegenContext): Boolean =
        codegenContext.settings.codegenConfig.includeFluentClient

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        if (!applies(codegenContext)) {
            return
        }

        val generics = if (codegenContext.smithyRuntimeMode.generateOrchestrator) {
            NoClientGenerics(codegenContext.runtimeConfig)
        } else {
            FlexibleClientGenerics(
                connectorDefault = null,
                middlewareDefault = null,
                retryDefault = RuntimeType.smithyClient(codegenContext.runtimeConfig).resolve("retry::Standard"),
                client = RuntimeType.smithyClient(codegenContext.runtimeConfig),
            )
        }

        FluentClientGenerator(
            codegenContext,
            generics = generics,
            customizations = listOf(GenericFluentClient(codegenContext)),
        ).render(rustCrate)
        rustCrate.withModule(ClientRustModule.Client.customize) {
            renderCustomizableOperationSend(codegenContext, generics, this)
        }

        rustCrate.mergeFeature(Feature("rustls", default = true, listOf("aws-smithy-client/rustls")))
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        if (!applies(codegenContext)) {
            return baseCustomizations
        }

        return baseCustomizations + object : LibRsCustomization() {
            override fun section(section: LibRsSection) = when (section) {
                is LibRsSection.Body -> writable {
                    rust("pub use client::{Client, Builder};")
                }
                else -> emptySection
            }
        }
    }
}

sealed class FluentClientSection(name: String) : Section(name) {
    /** Write custom code into an operation fluent builder's impl block */
    data class FluentBuilderImpl(
        val operationShape: OperationShape,
        val operationErrorType: Symbol,
    ) : FluentClientSection("FluentBuilderImpl")

    /** Write custom code into the docs */
    data class FluentClientDocs(val serviceShape: ServiceShape) : FluentClientSection("FluentClientDocs")
}

abstract class FluentClientCustomization : NamedCustomization<FluentClientSection>()

class GenericFluentClient(private val codegenContext: ClientCodegenContext) : FluentClientCustomization() {
    override fun section(section: FluentClientSection): Writable {
        return when (section) {
            is FluentClientSection.FluentClientDocs -> writable {
                val serviceName = codegenContext.serviceShape.serviceNameOrDefault("the service")
                docs(
                    """
                    An ergonomic client for $serviceName.

                    This client allows ergonomic access to $serviceName.
                    Each method corresponds to an API defined in the service's Smithy model,
                    and the request and response shapes are auto-generated from that same model.
                    """,
                )
                FluentClientDocs.clientConstructionDocs(codegenContext)(this)
                FluentClientDocs.clientUsageDocs(codegenContext)(this)
            }
            else -> emptySection
        }
    }
}
