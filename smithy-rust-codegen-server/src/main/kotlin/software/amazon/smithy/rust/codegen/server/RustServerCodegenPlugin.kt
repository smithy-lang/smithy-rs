/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.codegen.core.ReservedWordSymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWordSymbolProvider
import software.amazon.smithy.rust.codegen.core.BaseSymbolMetadataProvider
import software.amazon.smithy.rust.codegen.core.CodegenTarget
import software.amazon.smithy.rust.codegen.core.EventStreamSymbolProvider
import software.amazon.smithy.rust.codegen.core.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.StreamingShapeMetadataProvider
import software.amazon.smithy.rust.codegen.core.StreamingShapeSymbolProvider
import software.amazon.smithy.rust.codegen.core.SymbolVisitor
import software.amazon.smithy.rust.codegen.server.customizations.CustomValidationExceptionWithReasonDecorator
import software.amazon.smithy.rust.codegen.server.customizations.ServerRequiredCustomizations
import software.amazon.smithy.rust.codegen.server.customizations.SmithyValidationExceptionDecorator
import software.amazon.smithy.rust.codegen.server.customize.CombinedServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.testutil.ServerDecoratableBuildPlugin
import java.util.logging.Level
import java.util.logging.Logger

/**
 * Rust Server Codegen Plugin
 *
 * This is the entrypoint for code generation, triggered by the smithy-build plugin.
 * `resources/META-INF.services/software.amazon.smithy.build.SmithyBuildPlugin` refers to this class by name which
 * enables the smithy-build plugin to invoke `execute` with all Smithy plugin context + models.
 */
class RustServerCodegenPlugin : ServerDecoratableBuildPlugin() {
    private val logger = Logger.getLogger(javaClass.name)

    override fun getName(): String = "rust-server-codegen"

    /**
     * See [software.amazon.smithy.rust.codegen.client.RustClientCodegenPlugin].
     */
    override fun executeWithDecorator(
        context: PluginContext,
        vararg decorator: ServerCodegenDecorator,
    ) {
        Logger.getLogger(ReservedWordSymbolProvider::class.java.name).level = Level.OFF
        val codegenDecorator =
            CombinedServerCodegenDecorator.fromClasspath(
                context,
                ServerRequiredCustomizations(),
                SmithyValidationExceptionDecorator(),
                CustomValidationExceptionWithReasonDecorator(),
                *decorator,
            )
        logger.info("Loaded plugin to generate pure Rust bindings for the server SDK")
        ServerCodegenVisitor(context, codegenDecorator).execute()
    }

    companion object {
        /**
         * See [software.amazon.smithy.rust.codegen.client.RustClientCodegenPlugin].
         */
        fun baseSymbolProvider(
            settings: ServerRustSettings,
            model: Model,
            serviceShape: ServiceShape,
            rustSymbolProviderConfig: RustSymbolProviderConfig,
            constrainedTypes: Boolean = true,
            includeConstrainedShapeProvider: Boolean = true,
            codegenDecorator: ServerCodegenDecorator,
        ) = SymbolVisitor(settings, model, serviceShape = serviceShape, config = rustSymbolProviderConfig)
            // Generate public constrained types for directly constrained shapes.
            .let {
                if (includeConstrainedShapeProvider) ConstrainedShapeSymbolProvider(it, serviceShape, constrainedTypes) else it
            }
            // Generate different types for EventStream shapes (e.g. transcribe streaming)
            .let { EventStreamSymbolProvider(rustSymbolProviderConfig.runtimeConfig, it, CodegenTarget.SERVER) }
            // Generate [ByteStream] instead of `Blob` for streaming binary shapes (e.g. S3 GetObject)
            .let { StreamingShapeSymbolProvider(it) }
            // Add Rust attributes (like `#[derive(PartialEq)]`) to generated shapes
            .let { BaseSymbolMetadataProvider(it, additionalAttributes = listOf()) }
            // Constrained shapes generate newtypes that need the same derives we place on types generated from aggregate shapes.
            .let { ConstrainedShapeSymbolMetadataProvider(it, constrainedTypes) }
            // Streaming shapes need different derives (e.g. they cannot derive `PartialEq`)
            .let { StreamingShapeMetadataProvider(it) }
            // Derive `Eq` and `Hash` if possible.
            .let { DeriveEqAndHashSymbolMetadataProvider(it) }
            // Rename shapes that clash with Rust reserved words & and other SDK specific features e.g. `send()` cannot
            // be the name of an operation input
            .let { RustReservedWordSymbolProvider(it, ServerReservedWords) }
            // Allows decorators to inject a custom symbol provider
            .let { codegenDecorator.symbolProvider(it) }
            // Inject custom symbols.
            .let { CustomShapeSymbolProvider(it) }
    }
}
