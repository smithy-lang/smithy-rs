/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.CodegenContext
import software.amazon.smithy.rust.codegen.core.CodegenTarget
import software.amazon.smithy.rust.codegen.core.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.generators.BuilderInstantiator
import software.amazon.smithy.rust.codegen.server.generators.ServerBuilderInstantiator
import software.amazon.smithy.rust.codegen.server.generators.protocol.returnSymbolToParseFn

/**
 * [ServerCodegenContext] contains code-generation context that is _specific_ to the [RustServerCodegenPlugin] plugin
 * from the `rust-codegen-server` subproject.
 *
 * It inherits from [CodegenContext], which contains code-generation context that is common to _all_ smithy-rs plugins.
 *
 * This class has to live in the `codegen` subproject because it is referenced in common generators to both client
 * and server (like [JsonParserGenerator]).
 */
data class ServerCodegenContext(
    override val model: Model,
    override val symbolProvider: RustSymbolProvider,
    override val moduleDocProvider: ModuleDocProvider?,
    override val serviceShape: ServiceShape,
    override val protocol: ShapeId,
    override val settings: ServerRustSettings,
    val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    val constrainedShapeSymbolProvider: RustSymbolProvider,
    val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    val pubCrateConstrainedShapeSymbolProvider: PubCrateConstrainedShapeSymbolProvider,
) : CodegenContext(
        model, symbolProvider, moduleDocProvider, serviceShape, protocol, settings, CodegenTarget.SERVER,
    ) {
    override fun builderInstantiator(): BuilderInstantiator {
        return ServerBuilderInstantiator(symbolProvider, returnSymbolToParseFn(this))
    }
}
