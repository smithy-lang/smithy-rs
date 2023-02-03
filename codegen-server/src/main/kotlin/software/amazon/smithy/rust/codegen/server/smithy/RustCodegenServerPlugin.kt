/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.codegen.core.ReservedWordSymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWordSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.BaseSymbolMetadataProvider
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.EventStreamSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.StreamingShapeMetadataProvider
import software.amazon.smithy.rust.codegen.core.smithy.StreamingShapeSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.server.smithy.customizations.CustomValidationExceptionWithReasonDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customizations.ServerRequiredCustomizations
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customize.CombinedServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerDecoratableBuildPlugin
import java.util.logging.Level
import java.util.logging.Logger

/**
 * Rust Server Codegen Plugin
 *
 * This is the entrypoint for code generation, triggered by the smithy-build plugin.
 * `resources/META-INF.services/software.amazon.smithy.build.SmithyBuildPlugin` refers to this class by name which
 * enables the smithy-build plugin to invoke `execute` with all Smithy plugin context + models.
 */
class RustCodegenServerPlugin : ServerDecoratableBuildPlugin() {
    private val logger = Logger.getLogger(javaClass.name)

    override fun getName(): String = "rust-server-codegen"

    /**
     * See [software.amazon.smithy.rust.codegen.client.smithy.RustClientCodegenPlugin].
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
         * See [software.amazon.smithy.rust.codegen.client.smithy.RustClientCodegenPlugin].
         */
        fun baseSymbolProvider(
            model: Model,
            serviceShape: ServiceShape,
            symbolVisitorConfig: SymbolVisitorConfig,
            constrainedTypes: Boolean = true,
        ) =
            SymbolVisitor(model, serviceShape = serviceShape, config = symbolVisitorConfig)
                // Generate public constrained types for directly constrained shapes.
                .let { if (constrainedTypes) ConstrainedShapeSymbolProvider(it, model, serviceShape) else it }
                // Generate different types for EventStream shapes (e.g. transcribe streaming)
                .let { EventStreamSymbolProvider(symbolVisitorConfig.runtimeConfig, it, model, CodegenTarget.SERVER) }
                // Generate [ByteStream] instead of `Blob` for streaming binary shapes (e.g. S3 GetObject)
                .let { StreamingShapeSymbolProvider(it, model) }
                // Add Rust attributes (like `#[derive(PartialEq)]`) to generated shapes
                .let { BaseSymbolMetadataProvider(it, model, additionalAttributes = listOf()) }
                // Constrained shapes generate newtypes that need the same derives we place on types generated from aggregate shapes.
                .let { ConstrainedShapeSymbolMetadataProvider(it, model, constrainedTypes) }
                // Streaming shapes need different derives (e.g. they cannot derive `PartialEq`)
                .let { StreamingShapeMetadataProvider(it, model) }
                // Derive `Eq` and `Hash` if possible.
                .let { DeriveEqAndHashSymbolMetadataProvider(it, model) }
                // Rename shapes that clash with Rust reserved words & and other SDK specific features e.g. `send()` cannot
                // be the name of an operation input
                .let { RustReservedWordSymbolProvider(it, model) }
    }
}
