/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.codegen.core.ReservedWordSymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.ClientCustomizations
import software.amazon.smithy.rust.codegen.client.smithy.customizations.HttpAuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.HttpConnectorConfigDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.IdempotencyTokenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.NoAuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SensitiveOutputDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.StaticSdkFeatureTrackerDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.RequiredCustomizations
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointParamsDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointsDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.StalledStreamProtectionDecorator
import software.amazon.smithy.rust.codegen.client.testutil.ClientDecoratableBuildPlugin
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.NonExhaustive
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWordSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.BaseSymbolMetadataProvider
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.EventStreamSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.smithy.StreamingShapeMetadataProvider
import software.amazon.smithy.rust.codegen.core.smithy.StreamingShapeSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import java.util.logging.Level
import java.util.logging.Logger

/**
 * Rust Client Codegen Plugin
 *
 * This is the entrypoint for code generation, triggered by the smithy-build plugin.
 * `resources/META-INF.services/software.amazon.smithy.build.SmithyBuildPlugin` refers to this class by name which
 * enables the smithy-build plugin to invoke `execute` with all Smithy plugin context + models.
 */
class RustClientCodegenPlugin : ClientDecoratableBuildPlugin() {
    override fun getName(): String = "rust-client-codegen"

    override fun executeWithDecorator(
        context: PluginContext,
        vararg decorator: ClientCodegenDecorator,
    ) {
        // Suppress extremely noisy logs about reserved words
        Logger.getLogger(ReservedWordSymbolProvider::class.java.name).level = Level.OFF
        // Discover `RustCodegenDecorators` on the classpath. `RustCodegenDecorator` returns different types of
        // customizations. A customization is a function of:
        // - location (e.g. the mutate section of an operation)
        // - context (e.g. the of the operation)
        // - writer: The active RustWriter at the given location
        val codegenDecorator =
            CombinedClientCodegenDecorator.fromClasspath(
                context,
                ClientCustomizations(),
                RequiredCustomizations(),
                FluentClientDecorator(),
                EndpointsDecorator(),
                EndpointParamsDecorator(),
                AuthDecorator(),
                NoAuthDecorator(),
                HttpAuthDecorator(),
                HttpConnectorConfigDecorator(),
                SensitiveOutputDecorator(),
                IdempotencyTokenDecorator(),
                StalledStreamProtectionDecorator(),
                StaticSdkFeatureTrackerDecorator(),
                *decorator,
            )

        // ClientCodegenVisitor is the main driver of code generation that traverses the model and generates code
        ClientCodegenVisitor(context, codegenDecorator).execute()
    }

    companion object {
        /**
         * When generating code, smithy types need to be converted into Rust types—that is the core role of the symbol provider
         *
         * The Symbol provider is composed of a base [SymbolVisitor] which handles the core functionality, then is layered
         * with other symbol providers, documented inline, to handle the full scope of Smithy types.
         */
        fun baseSymbolProvider(
            settings: ClientRustSettings,
            model: Model,
            serviceShape: ServiceShape,
            rustSymbolProviderConfig: RustSymbolProviderConfig,
            codegenDecorator: ClientCodegenDecorator,
        ) = SymbolVisitor(settings, model, serviceShape = serviceShape, config = rustSymbolProviderConfig)
            // Generate different types for EventStream shapes (e.g. transcribe streaming)
            .let { EventStreamSymbolProvider(rustSymbolProviderConfig.runtimeConfig, it, CodegenTarget.CLIENT) }
            // Generate `ByteStream` instead of `Blob` for streaming binary shapes (e.g. S3 GetObject)
            .let { StreamingShapeSymbolProvider(it) }
            // Add Rust attributes (like `#[derive(PartialEq)]`) to generated shapes
            .let { BaseSymbolMetadataProvider(it, additionalAttributes = listOf(NonExhaustive)) }
            // Streaming shapes need different derives (e.g. they cannot derive `PartialEq`)
            .let { StreamingShapeMetadataProvider(it) }
            // Rename shapes that clash with Rust reserved words & and other SDK specific features e.g. `send()` cannot
            // be the name of an operation input
            .let { RustReservedWordSymbolProvider(it, ClientReservedWords) }
            // Allows decorators to inject a custom symbol provider
            .let { codegenDecorator.symbolProvider(it) }
    }
}
