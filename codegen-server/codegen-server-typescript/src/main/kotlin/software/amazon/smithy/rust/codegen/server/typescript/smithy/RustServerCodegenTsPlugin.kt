/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.build.SmithyBuildPlugin
import software.amazon.smithy.codegen.core.ReservedWordSymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWordSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.BaseSymbolMetadataProvider
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.EventStreamSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.server.smithy.ConstrainedShapeSymbolMetadataProvider
import software.amazon.smithy.rust.codegen.server.smithy.ConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.DeriveEqAndHashSymbolMetadataProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerReservedWords
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustSettings
import software.amazon.smithy.rust.codegen.server.smithy.customizations.CustomValidationExceptionWithReasonDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customizations.ServerRequiredCustomizations
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customize.CombinedServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.typescript.smithy.customizations.DECORATORS
import java.util.logging.Level
import java.util.logging.Logger

/**
 * Rust with Typescript bindings Codegen Plugin.
 * This is the entrypoint for code generation, triggered by the smithy-build plugin.
 * `resources/META-INF.services/software.amazon.smithy.build.SmithyBuildPlugin` refers to this class by name which
 * enables the smithy-build plugin to invoke `execute` with all of the Smithy plugin context + models.
 */
class RustServerCodegenTsPlugin : SmithyBuildPlugin {
    private val logger = Logger.getLogger(javaClass.name)

    override fun getName(): String = "rust-server-codegen-typescript"

    override fun execute(context: PluginContext) {
        // Suppress extremely noisy logs about reserved words
        Logger.getLogger(ReservedWordSymbolProvider::class.java.name).level = Level.OFF
        // Discover [RustCodegenDecorators] on the classpath. [RustCodegenDecorator] return different types of
        // customization. A customization is a function of:
        // - location (e.g. the mutate section of an operation)
        // - context (e.g. the of the operation)
        // - writer: The active RustWriter at the given location
        val codegenDecorator: CombinedServerCodegenDecorator =
            CombinedServerCodegenDecorator.fromClasspath(
                context,
                ServerRequiredCustomizations(),
                SmithyValidationExceptionDecorator(),
                CustomValidationExceptionWithReasonDecorator(),
                *DECORATORS,
            )

        // TsServerCodegenVisitor is the main driver of code generation that traverses the model and generates code
        logger.info("Loaded plugin to generate Rust/Node bindings for the server SSDK for projection ${context.projectionName}")
        TsServerCodegenVisitor(context, codegenDecorator).execute()
    }

    companion object {
        /**
         * When generating code, smithy types need to be converted into Rust types—that is the core role of the symbol provider
         *
         * The Symbol provider is composed of a base [SymbolVisitor] which handles the core functionality, then is layered
         * with other symbol providers, documented inline, to handle the full scope of Smithy types.
         */
        fun baseSymbolProvider(
            settings: ServerRustSettings,
            model: Model,
            serviceShape: ServiceShape,
            rustSymbolProviderConfig: RustSymbolProviderConfig,
            constrainedTypes: Boolean = true,
            includeConstrainedShapeProvider: Boolean = true,
            codegenDecorator: ServerCodegenDecorator,
        ) = TsServerSymbolVisitor(settings, model, serviceShape = serviceShape, config = rustSymbolProviderConfig)
            // Generate public constrained types for directly constrained shapes.
            // In the Typescript server project, this is only done to generate constrained types for simple shapes (e.g.
            // a `string` shape with the `length` trait), but these always remain `pub(crate)`.
            .let {
                if (includeConstrainedShapeProvider) ConstrainedShapeSymbolProvider(it, serviceShape, constrainedTypes) else it
            }
            // Generate different types for EventStream shapes (e.g. transcribe streaming)
            .let { EventStreamSymbolProvider(rustSymbolProviderConfig.runtimeConfig, it, CodegenTarget.SERVER) }
            // Add Rust attributes (like `#[derive(PartialEq)]`) to generated shapes
            .let { BaseSymbolMetadataProvider(it, additionalAttributes = listOf()) }
            // Constrained shapes generate newtypes that need the same derives we place on types generated from aggregate shapes.
            .let { ConstrainedShapeSymbolMetadataProvider(it, constrainedTypes) }
            // Streaming shapes need different derives (e.g. they cannot derive Eq)
            .let { TsStreamingShapeMetadataProvider(it) }
            // Derive `Eq` and `Hash` if possible.
            .let { DeriveEqAndHashSymbolMetadataProvider(it) }
            // Rename shapes that clash with Rust reserved words & and other SDK specific features e.g. `send()` cannot
            // be the name of an operation input
            .let { RustReservedWordSymbolProvider(it, ServerReservedWords) }
            // Allows decorators to inject a custom symbol provider
            .let { codegenDecorator.symbolProvider(it) }
    }
}
