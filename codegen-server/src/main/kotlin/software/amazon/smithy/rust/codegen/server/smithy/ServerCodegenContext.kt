/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderInstantiator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderInstantiator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.returnSymbolToParseFn

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
    /**
     * Whether operations use the schema runtime instead of legacy HTTP serde.
     *
     * The protocol itself is resolved at runtime through the service builder's
     * `ProtocolRegistry`; a service whose protocol has no registration fails loudly in `build()`.
     */
    val usesSchemaHttpSerde: Boolean
        get() = settings.codegenConfig.schemaSerde && runtimeConfig.httpVersion == HttpVersion.Http1x

    override fun builderInstantiator(): BuilderInstantiator {
        return ServerBuilderInstantiator(symbolProvider, returnSymbolToParseFn(this))
    }
}
