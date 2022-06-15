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
 * Configuration needed to generate the client for a given Service<->Protocol pair
 */
// TODO Rename to CoreCodegenContext
open class CodegenContext(
    /**
     * The smithy model.
     *
     * Note: This model may or not be pruned to the given service closure, so ensure that `serviceShape` is used as
     * an entry point.
     */
    open val model: Model,

    open val symbolProvider: RustSymbolProvider,

    /**
     * Entrypoint service shape for code generation
     */
    open val serviceShape: ServiceShape,
    /**
     * Smithy Protocol to generate, e.g. RestJson1
     */
    open val protocol: ShapeId,
    /**
     * Settings loaded from smithy-build.json
     */
    open val settings: RustSettings,

    /**
     * Server vs. Client codegen
     *
     * Some settings are dependent on whether server vs. client codegen is being invoked.
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

data class ClientCodegenContext(
    override val model: Model,
    override val symbolProvider: RustSymbolProvider,
    override val serviceShape: ServiceShape,
    override val protocol: ShapeId,
    override val settings: ClientRustSettings,
    override val target: CodegenTarget,
) : CodegenContext(
    model, symbolProvider, serviceShape, protocol, settings, target
)

