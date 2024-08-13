/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderInstantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructSettings

/**
 * [CodegenContext] contains code-generation context that is _common to all_  smithy-rs plugins.
 *
 * Code-generation context is pervasive read-only global data that gets passed around to the generators.
 *
 * If your data is specific to the `rust-client-codegen` client plugin, put it in [ClientCodegenContext] instead.
 * If your data is specific to the `rust-server-codegen` server plugin, put it in [ServerCodegenContext] instead.
 */
abstract class CodegenContext(
    /**
     * The smithy model.
     *
     * Note: This model may or not be pruned to the given service closure, so ensure that `serviceShape` is used as
     * an entry point.
     */
    open val model: Model,
    /**
     * The "canonical" symbol provider to convert Smithy [Shape]s into [Symbol]s, which have an associated [RustType].
     */
    open val symbolProvider: RustSymbolProvider,
    /**
     * Provider of documentation for generated Rust modules.
     */
    open val moduleDocProvider: ModuleDocProvider?,
    /**
     * Entrypoint service shape for code generation.
     */
    open val serviceShape: ServiceShape,
    /**
     * Shape indicating the protocol to generate, e.g. RestJson1.
     */
    open val protocol: ShapeId,
    /**
     * Settings loaded from `smithy-build.json`.
     */
    open val settings: CoreRustSettings,
    /**
     * Are we generating code for a smithy-rs client or server?
     *
     * Several code generators are reused by both the client and server plugins, but only deviate in small and contained
     * parts (e.g. changing a return type or adding an attribute).
     * Instead of splitting the generator in two or setting up an inheritance relationship, sometimes it's best
     * to just look up this flag.
     */
    open val target: CodegenTarget,
) {
    /**
     * Configuration of the runtime package:
     * - Where are the runtime crates (smithy-*) located on the file system? Or are they versioned?
     * - What are they called?
     *
     * This is just a convenience. To avoid typing `context.settings.runtimeConfig`, you can simply write
     * `context.runtimeConfig`.
     */
    val runtimeConfig: RuntimeConfig by lazy { settings.runtimeConfig }

    /**
     * The name of the cargo crate to generate e.g. `aws-sdk-s3`
     * This is loaded from the smithy-build.json during codegen.
     *
     * This is just a convenience. To avoid typing `context.settings.moduleName`, you can simply write
     * `context.moduleName`.
     */
    val moduleName: String by lazy { settings.moduleName }

    /**
     * A moduleName for a crate uses kebab-case. When you want to `use` a crate in Rust code,
     * it must be in snake-case. Call this method to get this crate's name in snake-case.
     */
    fun moduleUseName() = moduleName.replace("-", "_")

    /** Return a ModuleDocProvider or panic if one wasn't configured */
    fun expectModuleDocProvider(): ModuleDocProvider =
        checkNotNull(moduleDocProvider) {
            "A ModuleDocProvider must be set on the CodegenContext"
        }

    fun structSettings() = StructSettings(settings.codegenConfig.flattenCollectionAccessors)

    abstract fun builderInstantiator(): BuilderInstantiator
}
