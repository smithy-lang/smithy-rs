/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget

/**
 * [CoreCodegenContext] contains code-generation context that is _common to all_  smithy-rs plugins.
 *
 * Code-generation context is pervasive read-only global data that gets passed around to the generators.
 *
 * If your data is specific to the `rust-codegen` client plugin, put it in [ClientCodegenContext] instead.
 * If your data is specific to the `rust-server-codegen` server plugin, put it in [ServerCodegenContext] instead.
 */
open class CoreCodegenContext(
    /**
     * The smithy model.
     *
     * Note: This model may or not be pruned to the given service closure, so ensure that `serviceShape` is used as
     * an entry point.
     */
    open val model: Model,

    open val symbolProvider: RustSymbolProvider,

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
    open val settings: RustSettings,

    /**
     * Server vs. Client codegen
     *
     * Several code generators are reused by both the client and server plugins, but only deviate in small and contained
     * parts (e.g. changing a return type or adding an attribute).
     * Instead of splitting the generator in two or setting up an inheritance relationship, sometimes it's best
     * to just lookup this flag.
     */
    open val target: CodegenTarget,
) {

    // TODO This is just a convenience: we should remove this property and refactor all code to use the `runtimeConfig`
    //  inside `settings` directly.
    val runtimeConfig: RuntimeConfig by lazy { settings.runtimeConfig }

    /**
     * The name of the cargo crate to generate e.g. `aws-sdk-s3`
     * This is loaded from the smithy-build.json during codegen.
     */
    // TODO This is just a convenience: we should remove this property and refactor all code to use the `moduleName`
    //  inside `settings` directly.
    val moduleName: String by lazy { settings.moduleName }

    /**
     * A moduleName for a crate uses kebab-case. When you want to `use` a crate in Rust code,
     * it must be in snake-case. Call this method to get this crate's name in snake-case.
     */
    fun moduleUseName() = moduleName.replace("-", "_")
}
