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
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderInstantiator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolCodegenModules
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderInstantiator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.returnSymbolToParseFn

class ServerProtocolSelectionMetadata(protocolIds: List<ShapeId>) {
    val protocolIds: List<ShapeId> = protocolIds.toList()

    init {
        require(this.protocolIds.isNotEmpty()) { "At least one server protocol must be selected" }
        require(this.protocolIds.distinct().size == this.protocolIds.size) {
            "Selected server protocols must not contain duplicates"
        }
    }

    val isMultiProtocol: Boolean
        get() = protocolIds.size > 1

    val primaryProtocolId: ShapeId
        get() = protocolIds.first()
}

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
    val protocolSelectionMetadata: ServerProtocolSelectionMetadata =
        ServerProtocolSelectionMetadata(listOf(protocol)),
    override val protocolCodegenModules: ProtocolCodegenModules = ProtocolCodegenModules.Default,
) : CodegenContext(
        model,
        symbolProvider,
        moduleDocProvider,
        serviceShape,
        protocol,
        settings,
        CodegenTarget.SERVER,
        protocolCodegenModules,
    ) {
    /** Whether this server is generating more than one protocol. */
    val isMultiProtocol: Boolean
        get() = protocolSelectionMetadata.isMultiProtocol

    override fun builderInstantiator(): BuilderInstantiator {
        return ServerBuilderInstantiator(symbolProvider, returnSymbolToParseFn(this))
    }
}
