/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.build.SmithyBuildPlugin
import software.amazon.smithy.codegen.core.ReservedWordSymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute.Companion.NonExhaustive
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWordSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.customizations.ClientCustomizations
import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator
import java.util.logging.Level
import java.util.logging.Logger

/** Rust Codegen Plugin
 *  This is the entrypoint for code generation, triggered by the smithy-build plugin.
 *  `resources/META-INF.services/software.amazon.smithy.build.SmithyBuildPlugin` refers to this class by name which
 *  enables the smithy-build plugin to invoke `execute` with all of the Smithy plugin context + models.
 */
class RustCodegenPlugin : SmithyBuildPlugin {
    override fun getName(): String = "rust-codegen"

    override fun execute(context: PluginContext) {
        // Suppress extremely noisy logs about reserved words
        Logger.getLogger(ReservedWordSymbolProvider::class.java.name).level = Level.OFF
        // Discover `RustCodegenDecorators` on the classpath. `RustCodegenDecorator` return different types of
        // customization. A customization is a function of:
        // - location (e.g. the mutate section of an operation)
        // - context (e.g. the of the operation)
        // - writer: The active RustWriter at the given location
        val codegenDecorator = CombinedCodegenDecorator.fromClasspath(context, ClientCustomizations())

        // CodegenVisitor is the main driver of code generation that traverses the model and generates code
        CodegenVisitor(context, codegenDecorator).execute()
    }

    companion object {
        /** SymbolProvider
         * When generating code, smithy types need to be converted into Rust types—that is the core role of the symbol provider
         *
         * The Symbol provider is composed of a base `SymbolVisitor` which handles the core funcitonality, then is layered
         * with other symbol providers, documented inline, to handle the full scope of Smithy types.
         */
        fun baseSymbolProvider(model: Model, serviceShape: ServiceShape, symbolVisitorConfig: SymbolVisitorConfig = DefaultConfig) =
            SymbolVisitor(model, serviceShape = serviceShape, config = symbolVisitorConfig)
                // Generate different types for EventStream shapes (e.g. transcribe streaming)
                .let { EventStreamSymbolProvider(symbolVisitorConfig.runtimeConfig, it, model) }
                // Generate `ByteStream` instead of `Blob` for streaming binary shapes (e.g. S3 GetObject)
                .let { StreamingShapeSymbolProvider(it, model) }
                // Add Rust attributes (like `#[derive(PartialEq)]`) to generated shapes
                .let { BaseSymbolMetadataProvider(it, additionalAttributes = listOf(NonExhaustive)) }
                // Streaming shapes need different derives (e.g. they cannot derive Eq)
                .let { StreamingShapeMetadataProvider(it, model) }
                // Rename shapes that clash with Rust reserved words & and other SDK specific features e.g. `send()` cannot
                // be the name of an operation input
                .let { RustReservedWordSymbolProvider(it, model) }
    }
}
