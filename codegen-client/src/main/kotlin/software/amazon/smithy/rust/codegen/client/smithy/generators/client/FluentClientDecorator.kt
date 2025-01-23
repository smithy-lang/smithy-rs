/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.util.serviceNameOrDefault

/**
 * Generates the client via codegen decorator.
 *
 * > Why is this a decorator instead of a normal generator that gets called from the codegen visitor?
 *
 * The AWS SDK needs to make significant changes from what smithy-rs generates for generic clients,
 * and the easiest way to do that is to completely replace the client generator. With this as
 * a decorator, it can be excluded entirely and replaced in the sdk-codegen plugin.
 */
class FluentClientDecorator : ClientCodegenDecorator {
    override val name: String = "FluentClient"
    override val order: Byte = 0

    private fun applies(codegenContext: ClientCodegenContext): Boolean =
        codegenContext.settings.codegenConfig.includeFluentClient

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        if (!applies(codegenContext)) {
            return
        }

        FluentClientGenerator(
            codegenContext,
            customizations = listOf(GenericFluentClient(codegenContext)),
        ).render(rustCrate)

        rustCrate.mergeFeature(Feature("rustls", default = true, listOf("aws-smithy-runtime/tls-rustls")))
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        if (!applies(codegenContext)) {
            return baseCustomizations
        }

        return baseCustomizations +
            object : LibRsCustomization() {
                override fun section(section: LibRsSection) =
                    when (section) {
                        is LibRsSection.Body ->
                            writable {
                                rust("pub use client::Client;")
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

    /** Write custom code for adding additional client plugins to base_client_runtime_plugins */
    data class AdditionalBaseClientPlugins(val plugins: String, val config: String) :
        FluentClientSection("AdditionalBaseClientPlugins")

    /** Write additional code before plugins are configured */
    data class BeforeBaseClientPluginSetup(val config: String) :
        FluentClientSection("BeforeBaseClientPluginSetup")
}

abstract class FluentClientCustomization : NamedCustomization<FluentClientSection>()

class GenericFluentClient(private val codegenContext: ClientCodegenContext) : FluentClientCustomization() {
    override fun section(section: FluentClientSection): Writable {
        return when (section) {
            is FluentClientSection.FluentClientDocs ->
                writable {
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
                    FluentClientDocs.waiterDocs(codegenContext)(this)
                }
            else -> emptySection
        }
    }
}
